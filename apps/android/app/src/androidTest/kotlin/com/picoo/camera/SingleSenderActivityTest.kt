package com.picoo.camera

import androidx.lifecycle.ViewModelProvider
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-UI-003: notification/repeated launches must keep the active Sender owner. */
class SingleSenderActivityTest {
    @Test
    fun repeatedExplicitLaunchReusesActivityAndSenderSession() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val original = AtomicReference<MainActivity>()
        val session = AtomicReference<SenderSessionViewModel>()
        try {
            repeat(3) {
                val resumed = CountDownLatch(1)
                val returned = AtomicReference<MainActivity>()
                val callback = ActivityLifecycleCallback { activity, stage ->
                    if (activity is MainActivity && stage == Stage.RESUMED) {
                        returned.set(activity)
                        resumed.countDown()
                    }
                }
                val monitor = ActivityLifecycleMonitorRegistry.getInstance()
                monitor.addLifecycleCallback(callback)
                try {
                    // Use a real shell launch: ActivityScenario's synchronous new-Activity
                    // invoker cannot represent reuse of a singleTask Activity.
                    instrumentation.uiAutomation.executeShellCommand(
                        "am start -n ${instrumentation.targetContext.packageName}/com.picoo.camera.MainActivity",
                    ).use { descriptor ->
                        android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor)
                            .use { it.readBytes() }
                    }
                    assertTrue("activity did not resume", resumed.await(5, TimeUnit.SECONDS))
                    instrumentation.runOnMainSync {
                        val current = returned.get()
                        val model = ViewModelProvider(current)[SenderSessionViewModel::class.java]
                        if (original.get() == null) {
                            original.set(current)
                            session.set(model)
                        } else {
                            assertSame("second Activity would own a second Sender", original.get(), current)
                            assertSame(session.get(), model)
                        }
                    }
                } finally {
                    monitor.removeLifecycleCallback(callback)
                }
            }
        } finally {
            instrumentation.runOnMainSync { original.get()?.finish() }
        }
    }
}
