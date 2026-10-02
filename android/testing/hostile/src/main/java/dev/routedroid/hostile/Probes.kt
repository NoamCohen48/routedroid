package dev.routedroid.hostile

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import dev.routedroid.protocol.auth.BootstrapRecord

/**
 * What any installed app could try against the product. PASS means the attempt was
 * refused or had no effect, FAIL means it got through, INCONCLUSIVE means the outcome
 * says nothing either way (an unexpected error, or a VPN already up before the probe).
 */
class Probes(private val context: Context) {
    private enum class Verdict { PASS, FAIL, INCONCLUSIVE }

    private val uri = Uri.parse(BootstrapRecord.PROVIDER_URI)
    private val app = "dev.routedroid"

    fun run(report: (String) -> Unit) {
        fun probe(name: String, attempt: () -> Pair<Verdict, String>) {
            val (verdict, detail) = try {
                attempt()
            } catch (e: SecurityException) {
                Verdict.PASS to "denied: ${e.message?.take(80)}"
            } catch (e: Exception) {
                Verdict.INCONCLUSIVE to "${e.javaClass.simpleName}: ${e.message?.take(80)}"
            }
            report("HOSTILE-RESULT $name $verdict $detail")
        }

        probe("write-record") {
            // Well formed, so only the provider's permission can stop it.
            val record = BootstrapRecord.of("hostile", 9000, ByteArray(32)).encode()
            context.contentResolver.openOutputStream(uri, "w")!!.use { it.write(record) }
            Verdict.FAIL to "provider accepted a forged record"
        }
        for (mode in listOf("r", "rw")) probe("open-$mode") {
            context.contentResolver.openFileDescriptor(uri, mode)!!.close()
            Verdict.FAIL to "provider opened in mode $mode"
        }
        probe("call") {
            // call() is not covered by android:permission; it must stay the inert default.
            val reply = context.contentResolver.call(uri, "record", null, null)
            if (reply == null) Verdict.PASS to "call() returned null" else Verdict.FAIL to "call() returned $reply"
        }
        probe("stop-service") {
            val stop = Intent("dev.routedroid.action.STOP").setComponent(ComponentName(app, "$app.vpn.RoutedroidVpnService"))
            val started = context.startService(stop)
            if (started == null) Verdict.INCONCLUSIVE to "service not found" else Verdict.FAIL to "service accepted STOP"
        }
        probe("launch-no-vpn") { launches() }
    }

    /** No record matches these launches, so none may start a session (§7.2). */
    private fun launches(): Pair<Verdict, String> {
        if (vpnUp()) return Verdict.INCONCLUSIVE to "a VPN was already up"
        val bootstrap = ComponentName(app, "$app.bootstrap.BootstrapActivity")
        fun launch(extras: Intent.() -> Unit) =
            context.startActivity(Intent().setComponent(bootstrap).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK).apply(extras))
        launch {}
        launch { putExtra("session", 42) }
        launch { putExtra("session", "../../record") }
        launch { putExtra("session", "x".repeat(10_000)) }
        // The old LaunchGate let a burst like this lock the real host out (review A-2.2).
        repeat(20) { launch { putExtra("session", "guess-$it") } }
        Thread.sleep(3_000)
        return if (vpnUp()) Verdict.FAIL to "a VPN came up" else Verdict.PASS to "24 launches, no VPN"
    }

    private fun vpnUp(): Boolean {
        val cm = context.getSystemService(ConnectivityManager::class.java)
        @Suppress("DEPRECATION")
        return cm.allNetworks.any { cm.getNetworkCapabilities(it)?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true }
    }
}
