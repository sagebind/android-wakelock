//! Safe and ergonomic Rust bindings to the [Android WakeLock
//! API](https://developer.android.com/reference/android/os/PowerManager#newWakeLock(int,%20java.lang.String)).
//!
//! **Device battery life will be significantly affected by the use of this
//! API.** Do not acquire `WakeLock`s unless you really need them, use the
//! minimum levels possible, and be sure to release them as soon as possible.

#![warn(
    future_incompatible,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    unused,
    clippy::all,
)]

use jni::{
    errors::Result,
    objects::{GlobalRef, JObject, JValue},
    AttachGuard, JavaVM,
};

/// Create a new partial wake lock with the given tag.
///
/// This convenience function is equivalent to the following:
///
/// ```
/// WakeLock::builder(tag)
///     .level(Level::Partial)
///     .build();
/// ```
pub fn partial<T: Into<String>>(tag: T) -> Result<WakeLock> {
    WakeLock::builder(tag).build()
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
    /// ## Deprecated
    ///
    /// This option was deprecated in API level 33. Most applications should use
    /// `R.attr.turnScreenOn` or `Activity.setTurnScreenOn(boolean)` instead, as
    /// this prevents the previous foreground app from being resumed first when
    /// the screen turns on.
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
        let power_manager = env
            .call_method(
                unsafe { JObject::from_raw(ctx.context().cast()) },
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::from(&power_manager_service_id)],
            )?
            .l()?;

        let name = env.new_string(&self.tag)?;
        let mut flags = self.level as i32;

        if self.acquire_causes_wakeup {
            flags |= 0x10000000;
        }

        if self.on_after_release {
            flags |= 0x20000000;
        }

        // Create the wake lock.
        let result = env.call_method(
            &power_manager,
            "newWakeLock",
            "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
            &[JValue::from(flags), JValue::from(&name)],
        )?;

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
/// Any application using a `WakeLock` must request the
/// `android.permission.WAKE_LOCK` permission in an `<uses-permission>` element
/// of the application's manifest. Obtain a wake lock by calling
/// [`WakeLock::new`].
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
    /// ## Tags
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
    /// - avoid using `Class#getName()` or similar method since this class name
    ///   can be transformed by java optimizer and obfuscator tools.
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
        let result = env.call_method(&self.wake_lock, "isHeld", "()Z", &[])?;

        result.z()
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
    /// multiple times by the same thread. The wake lock is not released on the
    /// device until all acquired references have been released.
    pub fn acquire(&self) -> Result<Guard<'_>> {
        let mut env = self.vm.attach_current_thread()?;

        env.call_method(&self.wake_lock, "acquire", "()V", &[])?;

        log::info!("acquired: {}", self.tag);

        Ok(Guard {
            wake_lock: self.wake_lock.clone(),
            env,
            tag: &self.tag,
        })
    }
}

/// A guard for an acquired wake lock.
///
/// The wake lock is released automatically when the guard is dropped, but
/// panics if there is an error releasing the wake lock. If you want to handle
/// errors on release then you can call [`Guard::release`] instead.
///
/// The current thread will remain attached to the current JVM until the guard
/// is released.
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
        self.env
            .call_method(&self.wake_lock, "release", "()V", &[])?;

        log::info!("released: {}", self.tag);

        Ok(())
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let _ = self.release_one();
    }
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Level {
    Partial = 0x00000001,
    Full = 0x0000001a,
    ScreenDim = 0x00000006,
    ScreenBright = 0x0000000a,
}
