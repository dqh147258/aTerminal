plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
val configuredServer = providers.gradleProperty("terminalServerUrl").orElse(providers.environmentVariable("ATERMINAL_SERVER_URL")).orNull
val configuredCa = providers.gradleProperty("terminalServerCaFile").orElse(providers.environmentVariable("ATERMINAL_SERVER_CA_FILE")).orNull
val localServer = "https://192.168.0.36:7200"
fun caContents(path: String?) = path?.let { file(it).readText() }.orEmpty()
android {
    namespace = "com.yxf.aterminal"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.yxf.aterminal"
        minSdk = 25
        targetSdk = 35
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        versionCode = 1
        versionName = "0.1.0-prototype"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64", "x86") }
        buildConfigField("boolean", "TERMINAL_DEBUG", "false")
        buildConfigField("String", "DEFAULT_SERVER_URL", groovy.json.JsonOutput.toJson(configuredServer.orEmpty()))
        buildConfigField("String", "DEFAULT_SERVER_CA_PEM", groovy.json.JsonOutput.toJson(caContents(configuredCa)))
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].java.srcDir("../../../build/bindings")
    sourceSets["main"].jniLibs.srcDir("../../../build/android-jni")
    sourceSets["main"].assets.srcDir("../../../build/fixtures")
    buildFeatures { buildConfig = true }
    buildTypes {
        debug {
            buildConfigField("boolean", "TERMINAL_DEBUG", "true")
            val address = configuredServer ?: localServer
            val ca = configuredCa ?: rootProject.file("../../deploy/secrets/lan-ca.crt").takeIf { address == localServer && it.isFile }?.absolutePath
            buildConfigField("String", "DEFAULT_SERVER_URL", groovy.json.JsonOutput.toJson(address))
            buildConfigField("String", "DEFAULT_SERVER_CA_PEM", groovy.json.JsonOutput.toJson(caContents(ca)))
        }
        release { isMinifyEnabled = false }
    }
}
dependencies {
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("junit:junit:4.13.2")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("androidx.annotation:annotation:1.9.1")
    implementation("io.noties.markwon:core:4.6.2")
    implementation("io.noties.markwon:ext-strikethrough:4.6.2")
    implementation("io.noties.markwon:ext-tables:4.6.2")
    implementation("io.noties.markwon:ext-tasklist:4.6.2")
}
