plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
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
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].java.srcDir("../../../build/bindings")
    sourceSets["main"].jniLibs.srcDir("../../../build/android-jni")
    sourceSets["main"].assets.srcDir("../../../build/fixtures")
    buildFeatures { buildConfig = true }
    buildTypes {
        debug { buildConfigField("boolean", "TERMINAL_DEBUG", "true") }
        release { isMinifyEnabled = false }
    }
}
dependencies {
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("junit:junit:4.13.2")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("androidx.annotation:annotation:1.9.1")
}
