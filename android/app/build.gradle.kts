plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

val nativeAbis = providers.gradleProperty("wasmdroidAbis").orElse("arm64-v8a")
val runtimeEnabled = providers.gradleProperty("wasmdroidRuntime").orElse("false")
val rustProfile = providers.gradleProperty("wasmdroidRustProfile").orElse("debug")
val runtimeCompiler = providers.gradleProperty("wasmdroidCompiler").orElse("cranelift")
val prepareEngine by tasks.registering(Exec::class) {
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
    outputs.dir(layout.buildDirectory.dir("generated/uniffi"))
    outputs.dir(layout.buildDirectory.dir("generated/jniLibs"))
    commandLine("powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
        "tools/build-native.ps1", "-Abis", nativeAbis.get(), "-RuntimeEnabled", runtimeEnabled.get(), "-RustProfile", rustProfile.get(), "-Compiler", runtimeCompiler.get())
}

android {
    namespace = "dev.wasmdroid.app"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "dev.wasmdroid.app"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-spike"
        buildConfigField("boolean", "RUNTIME_ENABLED", runtimeEnabled.get())
        ndk { abiFilters += nativeAbis.get().split(',') }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    buildFeatures { compose = true; buildConfig = true }
    sourceSets["main"].java.srcDir(layout.buildDirectory.dir("generated/uniffi"))
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs"))
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { jniLibs.useLegacyPackaging = false }
}
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
androidComponents {
    beforeVariants(selector().withBuildType("release")) { variant ->
        // BUSL Singlepass is only an explicitly selected development experiment.
        // Production/distribution requires a separate license decision.
        if (runtimeEnabled.get() == "true" && runtimeCompiler.get() == "singlepass") {
            variant.enable = false
        }
    }
}
tasks.named("preBuild") { dependsOn(prepareEngine) }

dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.10.01"))
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("net.java.dev.jna:jna:5.18.1@aar")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
