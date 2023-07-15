//! Safe and ergonomic Rust bindings to the [Android WakeLock
//! API](https://developer.android.com/reference/android/os/PowerManager#newWakeLock(int,%20java.lang.String)).
//! Wake locks allow an app or service to keep an Android device's display or
//! processor awake in order to complete some work. For more information about
//! wake locks, see the official [Android
//! guide](https://developer.android.com/training/scheduling/wakelock).
//!
//! In short: **device battery life may be significantly affected by the use of
//! this API**. Do not acquire `WakeLock`s unless you really need them, use the
//! minimum levels possible, and be sure to release them as soon as possible.
//!
//! # Platform support
//!
//! This library should work with all Android API levels. It cannot be used on any
//! other operating system, of course.
//!
//! # Creating wake locks
//!
//! The simplest way to create a wake lock is to use the [`partial`] function,
//! which creates a [partial][`Level::Partial`] wake lock configured with
//! reasonable defaults. This is the lowest level of wake lock, and is the most
//! friendly to battery life while still keeping the device awake to perform
//! computation.
//!
//! If you want to create a wake lock with a different level or with different
//! flags, you can use [`WakeLock::builder`] to create a [`Builder`] that
//! provides methods for setting other supported wake lock options.
//!
//! Creating a wake lock from Rust is a somewhat expensive operation, so it is
//! better to create your wake locks up front and reuse them during your app's
//! runtime as needed instead of creating them on-demand.
//!
//! # Acquiring and releasing wake locks
//!
//! Wake locks remain dormant until they are acquired. To acquire a wake lock,
//! call [`acquire`][WakeLock::acquire] on the wake lock. This will return a
//! guard object that will keep the wake lock acquired until it is dropped:
//!
//! ```no_run
//! // Create the wake lock.
//! let wake_lock = android_wakelock::partial("myapp:mytag")?;
//!
//! // Start keeping the device awake.
//! let guard = wake_lock.acquire()?;
//!
//! // Do some work while the device is awake...
//!
//! // Release the wake lock to allow the device to sleep again.
//! drop(guard);
//!
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Multiple threads can share the same wake lock and acquire it concurrently. As
//! long as at least one thread has acquired the wake lock, the device will be
//! kept awake.
//!
//! ```no_run
//! use std::{sync::Arc, thread};
//!
//! // Create the wake lock.
//! let wake_lock = Arc::new(android_wakelock::partial("myapp:mytag")?);
//! let wake_lock_clone = wake_lock.clone();
//!
//! // Spawn multiple threads that use the same wake lock to keep the device awake
//! // while they do some work.
//! let worker1 = thread::spawn(move || {
//!     // Keep the device awake while this worker runs.
//!     let _guard = wake_lock_clone.acquire().unwrap();
//!
//!     // Do some work...
//! });
//! let worker2 = thread::spawn(move || {
//!     // Keep the device awake while this worker runs.
//!     let _guard = wake_lock.acquire().unwrap();
//!
//!     // Some more work...
//! });
//!
//! worker1.join().unwrap();
//! worker2.join().unwrap();
//!
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(
    future_incompatible,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    unused,
    clippy::all
)]

use std::fmt;

use jni::{
    objects::{GlobalRef, JObject, JValue},
    AttachGuard, JavaVM,
};

const ACQUIRE_CAUSES_WAKEUP: i32 = 0x10000000;
const ON_AFTER_RELEASE: i32 = 0x20000000;

/// An error returned by the wake lock API. A variety of errors can occur when
/// calling Android APIs, such as JNI errors, or exceptions actually thrown by the
/// API itself.
pub type Error = Box<dyn std::error::Error>;

type Result<T> = std::result::Result<T, Error>;

/// Create a new partial wake lock with the given tag.
///
/// This convenience function is equivalent to the following:
///
/// ```no_run
/// use android_wakelock::{Level, WakeLock};
///
/// # let tag = "myapp:mytag";
/// WakeLock::builder(tag)
///     .level(Level::Partial)
///     .build();
/// ```
pub fn partial<T: Into<String>>(tag: T) -> Result<WakeLock> {
    WakeLock::builder(tag).build()
}

/// Possible levels for a wake lock.
#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Level {
    /// Ensures that the CPU is running; the screen and keyboard backlight will
    /// be allowed to go off.
    ///
    /// If the user presses the power button, then the screen will be turned off
    /// but the CPU will be kept on until all partial wake locks have been
    /// released.
    Partial = 0x00000001,

    /// Ensures that the screen and keyboard backlight are on at full
    /// brightness.
    ///
    /// If the user presses the power button, then the wake lock will be
    /// implicitly released by the system, causing both the screen and the CPU
    /// to be turned off. Contrast with [`Level::Partial`].
    ///
    /// # Deprecation
    ///
    /// **This constant was deprecated in API level 17.** Most applications
    /// should use `WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON` instead of
    /// this type of wake lock, as it will be correctly managed by the platform
    /// as the user moves between applications and doesn't require a special
    /// permission.
    #[deprecated]
    Full = 0x0000001a,

    /// Ensures that the screen is on at full brightness; the keyboard backlight
    /// will be allowed to go off.
    ///
    /// If the user presses the power button, then the wake lock will be
    /// implicitly released by the system, causing both the screen and the CPU
    /// to be turned off. Contrast with [`Level::Partial`].
    ///
    /// # Deprecation
    ///
    /// **This constant was deprecated in API level 15.** Most applications
    /// should use `WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON` instead of
    /// this type of wake lock, as it will be correctly managed by the platform
    /// as the user moves between applications and doesn't require a special
    /// permission.
    #[deprecated]
    ScreenBright = 0x0000000a,

    /// Wake lock level: Ensures that the screen is on (but may be dimmed); the
    /// keyboard backlight will be allowed to go off.
    ///
    /// If the user presses the power button, then the wake lock will be
    /// implicitly released by the system, causing both the screen and the CPU
    /// to be turned off. Contrast with [`Level::Partial`].
    ///
    /// # Deprecation
    ///
    /// **This constant was deprecated in API level 17.** Most applications
    /// should use `WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON` instead of
    /// this type of wake lock, as it will be correctly managed by the platform
    /// as the user moves between applications and doesn't require a special
    /// permission.
    #[deprecated]
    ScreenDim = 0x00000006,
}

/// A builder for configuring and creating a wake lock.
#[derive(Clone, Debug)]
pub struct Builder {
    tag: String,
    level: Level,
    acquire_causes_wakeup: bool,
    on_after_release: bool,
}

impl Builder {
    /// Set the wake lock level.
    ///
    /// Generally [`Level::Partial`] wake locks are preferred, and is the
    /// default level if not specified. See [`Level`] for more information about
    /// the different available wake lock levels.
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Turn the screen on when the wake lock is acquired.
    ///
    /// This flag requires `Manifest.permission.TURN_SCREEN_ON` for apps
    /// targeting Android version `Build.VERSION_CODES#UPSIDE_DOWN_CAKE` and
    /// higher.
    ///
    /// Normally wake locks don't actually wake the device, they just cause the
    /// screen to remain on once it's already on. This flag will cause the
    /// device to wake up when the wake lock is acquired.
    ///
    /// Android TV playback devices attempt to turn on the HDMI-connected TV via
    /// HDMI-CEC on any wake-up, including wake-ups triggered by wake locks.
    ///
    /// Cannot be used with [`Level::Partial`].
    ///
    /// # Deprecation
    ///
    /// **This option was deprecated in API level 33.** Most applications should
    /// use `R.attr.turnScreenOn` or `Activity.setTurnScreenOn(boolean)`
    /// instead, as this prevents the previous foreground app from being resumed
    /// first when the screen turns on.
    #[deprecated]
    pub fn acquire_causes_wakeup(mut self, acquire_causes_wakeup: bool) -> Self {
        self.acquire_causes_wakeup = acquire_causes_wakeup;
        self
    }

    /// When this wake lock is released, poke the user activity timer so the
    /// screen stays on for a little longer.
    ///
    /// This will not turn the screen on if it is not already on.
    ///
    /// Cannot be used with [`Level::Partial`].
    pub fn on_after_release(mut self, on_after_release: bool) -> Self {
        self.on_after_release = on_after_release;
        self
    }

    /// Creates a new wake lock with the specified level and options.
    pub fn build(&self) -> Result<WakeLock> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }?;
        let mut env = vm.attach_current_thread()?;

        // Fetch the PowerManager system service.
        let power_manager_service_id = env.new_string("power")?;
        let power_manager = catch_exceptions(&mut env, |env| {
            env.call_method(
                unsafe { JObject::from_raw(ctx.context().cast()) },
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::from(&power_manager_service_id)],
            )?
            .l()
        })?;

        let name = env.new_string(&self.tag)?;
        let mut flags = self.level as i32;

        if self.acquire_causes_wakeup {
            flags |= ACQUIRE_CAUSES_WAKEUP;
        }

        if self.on_after_release {
            flags |= ON_AFTER_RELEASE;
        }

        // Create the wake lock.
        let result = catch_exceptions(&mut env, |env| {
            env.call_method(
                &power_manager,
                "newWakeLock",
                "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
                &[JValue::from(flags), JValue::from(&name)],
            )
        })?;

        let wake_lock = env.new_global_ref(result.l()?)?;

        drop(env);

        Ok(WakeLock {
            wake_lock,
            vm,
            tag: self.tag.clone(),
        })
    }
}

/// A wake lock is a mechanism to indicate that your application needs to have
/// the device stay on.
///
/// To obtain a wake lock, you can use [`WakeLock::builder`] to configure and
/// create a wake lock, or you can use [`partial`] to create a partial wake
/// lock configured with reasonable defaults.
///
/// Any application using a `WakeLock` must request the
/// `android.permission.WAKE_LOCK` permission in an `<uses-permission>` element
/// of the application's manifest.
#[derive(Debug)]
pub struct WakeLock {
    /// Reference to the underlying Java object.
    wake_lock: GlobalRef,

    /// The JVM the object belongs to.
    vm: JavaVM,

    /// The tag specified when the wake lock was created.
    tag: String,
}

impl WakeLock {
    /// Create a new builder with the given tag for configuring and creating a
    /// wake lock.
    ///
    /// # Tags
    ///
    /// Your class name (or other tag) for debugging purposes. Recommended
    /// naming conventions for tags to make debugging easier:
    ///
    /// - use a unique prefix delimited by a colon for your app/library (e.g.
    ///   `gmail:mytag`) to make it easier to understand where the wake locks
    ///   comes from. This namespace will also avoid collision for tags inside
    ///   your app coming from different libraries which will make debugging
    ///   easier.
    /// - use constants (e.g. do not include timestamps in the tag) to make it
    ///   easier for tools to aggregate similar wake locks. When collecting
    ///   debugging data, the platform only monitors a finite number of tags,
    ///   using constants will help tools to provide better debugging data.
    /// - avoid wrapping the tag or a prefix to avoid collision with wake lock
    ///   tags from the platform (e.g. `*alarm*`).
    /// - never include personally identifiable information for privacy reasons.
    pub fn builder<T: Into<String>>(tag: T) -> Builder {
        Builder {
            tag: tag.into(),
            level: Level::Partial,
            acquire_causes_wakeup: false,
            on_after_release: false,
        }
    }

    /// Returns true if the wake lock has outstanding references not yet
    /// released.
    pub fn is_held(&self) -> Result<bool> {
        let mut env = self.vm.attach_current_thread()?;

        catch_exceptions(&mut env, |env| {
            env.call_method(&self.wake_lock, "isHeld", "()Z", &[])?.z()
        })
    }

    /// Acquire the wake lock and force the device to stay on at the level that
    /// was requested when the wake lock was created.
    ///
    /// Returns a [`Guard`] which can be used to release the lock. You should
    /// release wake locks when you are done and don't need the lock anymore. It
    /// is very important to do this as soon as possible to avoid running down
    /// the device's battery excessively.
    ///
    /// Wake locks are reference counted like a semaphore and may be acquired
    /// multiple times by the same or a different thread. The wake lock is not
    /// released on the device until all acquired references have been released.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Create the wake lock.
    /// let wake_lock = android_wakelock::partial("myapp:mytag")?;
    ///
    /// // Start keeping the device awake.
    /// let guard = wake_lock.acquire()?;
    ///
    /// // Do some work while the device is awake...
    ///
    /// // Release the wake lock to allow the device to sleep again.
    /// drop(guard);
    ///
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn acquire(&self) -> Result<Guard<'_>> {
        let mut env = self.vm.attach_current_thread()?;

        catch_exceptions(&mut env, |env| {
            env.call_method(&self.wake_lock, "acquire", "()V", &[])
        })?;

        log::debug!("acquired wake lock \"{}\"", self.tag);

        Ok(Guard {
            wake_lock: self.wake_lock.clone(),
            env,
            tag: &self.tag,
        })
    }
}

/// A guard for an acquired wake lock.
///
/// To create a guard see [`WakeLock::acquire`].
///
/// The wake lock is released automatically when the guard is dropped, but
/// panics if there is an error releasing the wake lock. If you want to handle
/// errors on release then you can call [`Guard::release`] instead.
///
/// The current thread will remain attached to the current JVM until the guard
/// is released. The guard cannot be sent between threads.
pub struct Guard<'a> {
    /// Reference to the underlying Java object.
    wake_lock: GlobalRef,

    env: AttachGuard<'a>,

    /// The tag specified when the wake lock was created.
    tag: &'a str,
}

impl Guard<'_> {
    /// Releases the wake lock, returning an error if the underlying API threw
    /// an exception.
    pub fn release(mut self) -> Result<()> {
        self.release_one()
    }

    fn release_one(&mut self) -> Result<()> {
        catch_exceptions(&mut self.env, |env| {
            env.call_method(&self.wake_lock, "release", "()V", &[])?;

            log::debug!("released wake lock \"{}\"", self.tag);

            Ok(())
        })
    }
}

impl fmt::Debug for Guard<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Guard")
            .field("wake_lock", &self.wake_lock)
            .field("tag", &self.tag)
            .finish()
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.release_one() {
            panic!("error releasing wake lock \"{}\" on drop: {}", self.tag, e);
        }
    }
}

/// Helper for handling Java exceptions thrown when entering Java code that turns
/// thrown exceptions into formatted Rust errors.
#[inline]
fn catch_exceptions<'a, T, F>(env: &mut jni::JNIEnv<'a>, f: F) -> Result<T>
where
    F: FnOnce(&mut jni::JNIEnv<'a>) -> jni::errors::Result<T>,
{
    match f(env) {
        Ok(value) => Ok(value),
        Err(e @ jni::errors::Error::JavaException) => Err({
            if let Ok(exception) = env.exception_occurred() {
                let _ = env.exception_clear();

                env.call_method(exception, "getMessage", "()Ljava/lang/String;", &[])
                    .and_then(|value| value.l())
                    .and_then(|message| {
                        env.get_string(&message.into())
                            .map(|s| s.to_string_lossy().into_owned())
                    })
                    .map(|message| message.into())
                    .unwrap_or_else(|_| e.into())
            } else {
                e.into()
            }
        }),
        Err(e) => Err(e.into()),
    }
}
