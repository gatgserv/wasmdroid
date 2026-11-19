plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val nativeAbis = providers.gradleProperty("wasmdroidAbis").orElse("arm64-v8a")
val runtimeEnabled = providers.gradleProperty("wasmdroidRuntime").orElse("false")
val rustProfile = providers.gradleProperty("wasmdroidRustProfile").orElse("debug")
val runtimeCompiler = providers.gradleProperty("wasmdroidCompiler").orElse("cranelift")

abstract class PrepareAppAssets : Sync() {
    @get:OutputDirectory abstract val generatedAssets: DirectoryProperty
}

abstract class PrepareEngine : Exec() {
    @get:OutputDirectory abstract val generatedKotlin: DirectoryProperty
    @get:OutputDirectory abstract val generatedJniLibs: DirectoryProperty
}

val prepareAppAssets = tasks.register<PrepareAppAssets>("prepareAppAssets") {
    from("../../LICENSE") { rename { "LICENSE.txt" } }
    from("../../docs/privacy-policy.txt")
    from("../../THIRD_PARTY_NOTICES.md")
    from("../../third_party/vt100/LICENSE") { into("licenses/vt100") }
    from("../../engine/drivers/wasmer-local/vendor/wasmer-wasix/LICENSE") { into("licenses/wasmer-wasix") }
    from("../../engine/drivers/wasmer-local/vendor/virtual-net/LICENSE") { into("licenses/virtual-net") }
    from("../../third_party/saphyr-parser") { into("licenses/saphyr-parser") }
    from("../../third_party/shell-words") { into("licenses/shell-words") }
    from("../../third_party/arraydeque") { into("licenses/arraydeque") }
    from("../../third_party/zxing") { into("licenses/zxing") }
    generatedAssets.set(layout.buildDirectory.dir("generated/appAssets"))
    into(generatedAssets)
}
val prepareEngine = tasks.register<PrepareEngine>("prepareEngine") {
    workingDir(rootProject.projectDir.parentFile)
    // Explicit task inputs: the generated Kotlin and native library must match.
    inputs.files(fileTree("../../engine") { exclude("**/target/**") })
    inputs.files("../../Cargo.toml", "../../Cargo.lock", "../../tools/build-native.ps1")
    inputs.dir("../../fixtures")
    inputs.dir("../../tools/bindgen")
    inputs.file("../../rust-toolchain.toml")
    inputs.property("abis", nativeAbis)
    inputs.property("runtime", runtimeEnabled)
    inputs.property("rustProfile", rustProfile)
    inputs.property("compiler", runtimeCompiler)
    generatedKotlin.set(layout.buildDirectory.dir("generated/uniffi"))
    generatedJniLibs.set(layout.buildDirectory.dir("generated/jniLibs"))
    commandLine("powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
        "tools/build-native.ps1", "-Abis", nativeAbis.get(), "-RuntimeEnabled", runtimeEnabled.get(), "-RustProfile", rustProfile.get(), "-Compiler", runtimeCompiler.get())
}

android {
    namespace = "dev.wasmdroid.app"
    compileSdk = 37
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "ro.atgserv.wasmdroid"
        minSdk = 28
        targetSdk = 37
        versionCode = 2
        versionName = "0.1.1"
        buildConfigField("boolean", "RUNTIME_ENABLED", runtimeEnabled.get())
        ndk { abiFilters += listOf("arm64-v8a", "x86_64").intersect(nativeAbis.get().split(',').toSet()) }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    buildTypes {
        release {
            optimization {
                enable = true
            }
            proguardFiles("proguard-rules.pro")
        }
    }
    buildFeatures { compose = true; buildConfig = true }
    androidResources {
        // Keep dependency UI/accessibility translations out of this English-only app.
        localeFilters += listOf("en")
    }
    bundle { language { enableSplit = false } }
    // Unverified recipe fixtures belong only to the explicit acceptance APK.
    sourceSets["androidTest"].assets.directories.add("../../examples/catalog-seeds/notes")
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { jniLibs.useLegacyPackaging = false }
}
androidComponents {
    onVariants { variant ->
        variant.sources.kotlin!!.addGeneratedSourceDirectory(prepareEngine, PrepareEngine::generatedKotlin)
        variant.sources.jniLibs!!.addGeneratedSourceDirectory(prepareEngine, PrepareEngine::generatedJniLibs)
        variant.sources.assets!!.addGeneratedSourceDirectory(prepareAppAssets, PrepareAppAssets::generatedAssets)
    }
    beforeVariants(selector().withBuildType("release")) { variant ->
        // BUSL Singlepass is only an explicitly selected development experiment.
        // Production/distribution requires a separate license decision.
        if (runtimeEnabled.get() == "true" && runtimeCompiler.get() == "singlepass") {
            variant.enable = false
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("androidx.webkit:webkit:1.18.0")
    implementation("com.google.zxing:core:3.5.4")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
