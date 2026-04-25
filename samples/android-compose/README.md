# RPort Android Compose Sample

Minimal working Kotlin Multiplatform Android sample demonstrating:

1. Jetpack Compose UI
2. Live console output view
3. Plot render view
4. UniFFI generated RPort bindings integration
5. Evaluating R code and rendering plots

## Features
- ✅ Native R engine running directly on Android
- ✅ Interactive R code execution
- ✅ Real-time console output streaming
- ✅ PNG plot rendering with Coil
- ✅ Clean Material 3 interface

## Demo Plots

The renderer path is intentionally small but useful for Android demos:

```r
plot(c(1, 2, 3, 4), c(1, 4, 9, 16), type = "l", col = "blue", lwd = 2,
     main = "Quadratic growth", xlab = "x", ylab = "x^2")
plot(c(1, 2, 3), c(3, 1, 2), type = "p", col = "green", cex = 1.4,
     main = "Point sample", xlab = "group", ylab = "value")
```

## Build
```bash
./gradlew assembleDebug
```
