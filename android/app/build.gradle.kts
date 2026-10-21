import java.util.concurrent.TimeUnit

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
    // 2.0.0-alpha05.1 is the newest Paparazzi that still runs on AGP 8.13 (pre-AGP 9).
    id("app.cash.paparazzi") version "2.0.0-alpha05.1"
}

fun envOrProp(name: String): String? {
    val fromEnv = System.getenv(name)?.trim().orEmpty()
    if (fromEnv.isNotEmpty()) return fromEnv
    return (findProperty(name) as String?)?.trim()?.takeIf { it.isNotEmpty() }
}

/** Short git sha when the build has one. Release tags still own versionName. */
fun buildCommit(): String? {
    val raw = envOrProp("MEWO_COMMIT") ?: gitShortCommit() ?: return null
    val sha = raw.lowercase()
    if (!sha.matches(Regex("[0-9a-f]{7,40}"))) return null
    return sha.take(7)
}

fun gitShortCommit(): String? {
    return try {
        val process = ProcessBuilder("git", "rev-parse", "--short=7", "HEAD")
            .directory(rootProject.projectDir)
            .redirectError(ProcessBuilder.Redirect.DISCARD)
            .start()
        val text = process.inputStream.bufferedReader().readText().trim()
        val finished = process.waitFor(5, TimeUnit.SECONDS)
        if (!finished) {
            process.destroyForcibly()
            null
        } else if (process.exitValue() == 0 && text.isNotEmpty()) {
            text
        } else {
            null
        }
    } catch (_: Exception) {
        null
    }
}

val releaseStoreFile = envOrProp("MEWO_STORE_FILE")
val releaseStorePassword = envOrProp("MEWO_STORE_PASSWORD")
val releaseKeyAlias = envOrProp("MEWO_KEY_ALIAS")
val releaseKeyPassword = envOrProp("MEWO_KEY_PASSWORD")
val releaseSigningConfigured = releaseStoreFile != null
if (releaseSigningConfigured) {
    require(
        releaseStorePassword != null &&
            releaseKeyAlias != null &&
            releaseKeyPassword != null,
    ) {
        "MEWO_STORE_FILE is set. Also set MEWO_STORE_PASSWORD, MEWO_KEY_ALIAS, and MEWO_KEY_PASSWORD."
    }
}

android {
    namespace = "com.mewo.reader"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.mewo.reader"
        minSdk = 26
        targetSdk = 36
        versionCode = envOrProp("MEWO_VERSION_CODE")?.toInt() ?: 1
        // Release APKs set MEWO_VERSION_NAME from the v* tag in android-apk.yml.
        versionName = envOrProp("MEWO_VERSION_NAME") ?: "0.1.1"
        buildConfigField("String", "COMMIT", "\"${buildCommit().orEmpty()}\"")
    }

    signingConfigs {
        if (releaseSigningConfigured) {
            create("release") {
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            if (releaseSigningConfigured) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
        unitTests.isReturnDefaultValues = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    val readium = "3.3.0"

    implementation("org.readium.kotlin-toolkit:readium-shared:$readium")
    implementation("org.readium.kotlin-toolkit:readium-streamer:$readium")

    implementation(platform("androidx.compose:compose-bom:2025.08.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")

    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.2")
    implementation("androidx.lifecycle:lifecycle-process:2.9.2")
    implementation("androidx.navigation:navigation-compose:2.9.3")
    implementation("androidx.core:core-ktx:1.16.0")

    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation(platform("androidx.compose:compose-bom:2025.08.00"))
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    testImplementation("org.robolectric:robolectric:4.14.1")

    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}
