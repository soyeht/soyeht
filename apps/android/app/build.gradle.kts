plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.soyeht.next.android"
    compileSdk = 35

    defaultConfig {
        // Every machine identifier of the rewrite contains "next" (ADR 0112); this is the Dev profile.
        applicationId = "com.soyeht.next.dev.android"
        minSdk = providers.gradleProperty("soyeht.minSdk").get().toInt()
        targetSdk = 35
        versionCode = 1
        versionName = providers.gradleProperty("soyehtVersion").getOrElse("0.0.0")
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    sourceSets {
        // The core's Kotlin bindings and libraries are built by scripts/check_android.sh, never committed.
        getByName("main") {
            kotlin.srcDir(layout.buildDirectory.dir("generated/uniffi"))
            jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs"))
        }
    }
    testOptions { unitTests.isReturnDefaultValues = true }
}

// The tests that call the core load the library scripts/check_android.sh built for this computer.
tasks.withType<Test>().configureEach {
    systemProperty("jna.library.path", providers.environmentVariable("SOYEHT_CORE_LIB_DIR").getOrElse(""))
    systemProperty("soyeht.contracts", rootProject.projectDir.resolve("../../contracts").path)
}

dependencies {
    val compose = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(compose)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    // The bindings of the core call its library through JNA.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    // Pairing by camera: CameraX shows and feeds the picture, ZXing reads the code on the phone itself.
    val camera = "1.4.1"
    implementation("androidx.camera:camera-core:$camera")
    implementation("androidx.camera:camera-camera2:$camera")
    implementation("androidx.camera:camera-lifecycle:$camera")
    implementation("androidx.camera:camera-view:$camera")
    implementation("com.google.zxing:core:3.5.3")
    testImplementation("junit:junit:4.13.2")
    testImplementation("net.java.dev.jna:jna:5.14.0")
    testImplementation("org.json:json:20240303")
}
