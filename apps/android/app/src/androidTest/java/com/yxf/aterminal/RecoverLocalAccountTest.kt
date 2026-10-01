package com.yxf.aterminal

import android.os.Build
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.File
import java.net.URL
import java.security.KeyStore
import java.security.cert.CertificateFactory
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManagerFactory

/** Opt-in recovery after a test lost a rotated refresh token. Keeps the stored device key/ID.
 * Stop MainActivity before instrumentation. Private local-login-fixture.json supplies the existing
 * local account password; neither the saved session, request nor response is written to evidence.
 */
class RecoverLocalAccountTest {
    @Test fun recoverExpiredStoredLocalDevice() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val fixtureFile = File(context.filesDir, "local-login-fixture.json")
        assumeTrue("Explicit private local-login fixture required", fixtureFile.isFile)
        assertTrue(BuildConfig.TERMINAL_DEBUG)
        val fixture = JSONObject(fixtureFile.readText())
        val store = PairingStore(context, "account")
        val saved = JSONObject(store.load() ?: error("Existing device identity required"))
        val tokens = saved.getJSONObject("tokens")
        val server = saved.getString("server")
        assertEquals("https://192.168.0.36:7200", server)
        assertEquals(server, fixture.getString("server"))
        assertEquals(tokens.getString("username"), fixture.getString("username"))
        assertTrue("Use recovery only for an expired saved access token", tokens.getLong("expires_at") <= System.currentTimeMillis() / 1000)
        val originalDevice = tokens.getString("device_id")
        val originalIdentity = saved.getJSONObject("identity").toString()
        val certificates = CertificateFactory.getInstance("X.509")
            .generateCertificates(ByteArrayInputStream(BuildConfig.DEFAULT_SERVER_CA_PEM.toByteArray()))
        assertFalse("Bundled local CA required", certificates.isEmpty())
        val trust = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null)
            certificates.forEachIndexed { index, certificate -> setCertificateEntry("local-$index", certificate) }
        }
        val managers = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm()).apply { init(trust) }
        val tls = SSLContext.getInstance("TLS").apply { init(null, managers.trustManagers, null) }
        val request = JSONObject().put("username", fixture.getString("username"))
            .put("password", fixture.getString("password")).put("device_name", Build.MODEL)
            .put("platform", "android").put("public_key", saved.getJSONObject("identity").getString("public"))
        val connection = URL("$server/v2/auth/login").openConnection() as HttpsURLConnection
        val renewed = try {
            connection.sslSocketFactory = tls.socketFactory
            connection.connectTimeout = 20000; connection.readTimeout = 20000
            connection.requestMethod = "POST"; connection.doOutput = true
            connection.setRequestProperty("Content-Type", "application/json")
            connection.outputStream.use { it.write(request.toString().toByteArray(Charsets.UTF_8)) }
            assertEquals("Existing local-account authentication failed", 200, connection.responseCode)
            JSONObject(connection.inputStream.bufferedReader().use { it.readText() })
        } finally { connection.disconnect() }
        assertEquals("Recovery must preserve the exact device ID", originalDevice, renewed.getString("device_id"))
        assertEquals(tokens.getString("username"), renewed.getString("username"))
        saved.put("tokens", renewed)
        assertEquals(originalIdentity, saved.getJSONObject("identity").toString())
        store.save(saved.toString())
        File(context.filesDir, "local-device-recovery-results.json").writeText(JSONObject()
            .put("passed", true).put("device_id", originalDevice).put("identity_preserved", true)
            .put("server", server).toString())
    }
}
