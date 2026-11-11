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
            // The license documents of the repository root travel in the APK as assets (ADR 0149).
            assets.srcDir(layout.buildDirectory.dir("generated/licenses"))
            // The approved fonts and the providers' logos, copied from the Mac's and the shared package's resources below.
            res.srcDir(layout.buildDirectory.dir("generated/shared-res"))
        }
    }
    testOptions { unitTests.isReturnDefaultValues = true }
}

// LICENSE, NOTICE and the generated THIRD-PARTY-NOTICES.txt are copied from the repository root, so
// the app holds no second copy of them.
val copyLicenses by tasks.registering(Copy::class) {
    from(rootProject.projectDir.resolve("../..")) { include("LICENSE", "NOTICE", "THIRD-PARTY-NOTICES.txt") }
    into(layout.buildDirectory.dir("generated/licenses"))
}
tasks.named("preBuild") { dependsOn(copyLicenses) }

// The fonts of `contracts/design-tokens.json` (Nunito, Inter, JetBrains Mono) are the files the Mac app ships; the
// app holds no second copy. An Android resource name is lowercase with underscores.
val copyFonts by tasks.registering(Copy::class) {
    from(rootProject.projectDir.resolve("../mac/Resources/Fonts")) {
        include(
            "Nunito-Regular.ttf", "Nunito-SemiBold.ttf", "Nunito-Bold.ttf", "Inter.ttf",
            "JetBrainsMono-Regular.ttf", "JetBrainsMono-Medium.ttf", "JetBrainsMono-Bold.ttf",
        )
        rename { it.lowercase().replace("-", "_") }
    }
    into(layout.buildDirectory.dir("generated/shared-res/font"))
}
tasks.named("preBuild") { dependsOn(copyFonts) }

// The logos of the four providers are the files the Mac and the iPhone draw (the shared package's asset catalog),
// copied flat and named for Android; they are tinted in a palette color, as `ProviderGlyph` does.
val copyProviderIcons by tasks.registering(Copy::class) {
    from(rootProject.projectDir.resolve("../shared/Sources/SoyehtClient/Resources/ProviderIcons.xcassets")) {
        include("*.imageset/*.png")
        eachFile { relativePath = RelativePath(true, "provider_$name") }
        includeEmptyDirs = false
    }
    into(layout.buildDirectory.dir("generated/shared-res/drawable-nodpi"))
}
tasks.named("preBuild") { dependsOn(copyProviderIcons) }

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
    // Handwriting recognition on the device, below Android 14 (ADR 0157): the language model is downloaded only when asked for.
    implementation("com.google.mlkit:digital-ink-recognition:18.1.0")
    // ML Kit's model download runs on WorkManager. The app's manifest removes WorkManager's own start-up entry, so the
    // app starts it (`MlKitInkEngine.start`) only after the person turns the recognizer on; this is the version ML Kit brings.
    implementation("androidx.work:work-runtime:2.7.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("net.java.dev.jna:jna:5.14.0")
    testImplementation("org.json:json:20240303")
}
