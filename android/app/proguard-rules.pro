# JNI looks up JNA fields and methods by name. UniFFI also provides callback
# interfaces and structure fields that JNA finds through reflection.
-keep class com.sun.jna.** { *; }
-keep class dev.wasmdroid.engine.** { *; }

# JNA's optional desktop window handle APIs reference AWT. WasmDroid does not
# use those APIs on Android.
-dontwarn java.awt.Component
-dontwarn java.awt.GraphicsEnvironment
-dontwarn java.awt.HeadlessException
-dontwarn java.awt.Window
