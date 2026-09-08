export type Example = {
  id: string
  title: string
  category: string
  description: string
  mode: "plot" | "console"
  code: string
  color: string
}

const darkPlotDefaults = `par(bg = "#171e1c", fg = "#d7e1db",
     col.axis = "#b7c8bf", col.lab = "#b7c8bf", col.main = "#e2eee7")
`

/**
 * Return the source used by the gallery preview and playground for a theme.
 * Dark variants only adjust explicit gallery-owned colors; user code is never
 * rewritten through this helper.
 */
export function getExampleCode(example: Example, dark = false): string {
  if (!dark || example.mode !== "plot") return example.code
  if (example.id === "grid" || example.id === "woven-rosette") {
    return example.code
      .replaceAll("#f8f6ef", "#171e1c")
      .replaceAll("#52695e", "#9fb4a8")
      .replaceAll("#244e44", "#d7e8de")
  }
  const code = example.code
    .replaceAll('border = "white"', 'border = "#30403b"')
    .replaceAll("#faf3e6", "#171e1c")
  return `${darkPlotDefaults}${code}`
}

export const examples: Example[] = [
  {
    id: "loess",
    title: "Find the signal",
    category: "Statistics",
    description: "A little noise. A beautiful fit. Smooth a curve with LOESS.",
    mode: "plot",
    color: "#ce5736",
    code: `# A curve hiding in the noise
set.seed(42)
x <- seq(0, 10, length.out = 80)
y <- sin(x) + rnorm(80, sd = 0.22)
fit <- loess(y ~ x, span = 0.3)

plot(x, y, pch = 16, cex = 0.7,
     col = "#a8b6ae", xlab = "Time", ylab = "Signal",
     main = "A little less noise")
lines(x, predict(fit), col = "#cc5636", lwd = 3)`,
  },
  {
    id: "sunflower",
    title: "Watch averages settle",
    category: "Simulation",
    description:
      "Start with skewed data. Repeated samples reveal why averages become easier to predict.",
    mode: "plot",
    color: "#43877b",
    code: `# The central limit theorem, seen through repeated experiments
set.seed(42)
means <- function(n) {
  result <- numeric(600)
  for (i in 1:600) result[i] <- mean(rexp(n))
  result
}
a <- means(1)
b <- means(5)
c <- means(30)
x <- seq(0, 4, length.out = 240)
plot(x, dnorm(x, 1, 1), type = "n", ylim = c(0, 2.4),
     main = "More observations. Less uncertainty.",
     xlab = "Sample average", ylab = "Density")
colors <- c("#cf9477", "#77a798", "#386d66")
for (i in 1:3) {
  values <- list(a, b, c)[[i]]
  # Gaussian kernel density, evaluated directly
  bandwidth <- 1.06 * sd(values) * length(values)^(-0.2)
  estimate <- numeric(length(x))
  for (j in seq_along(x)) {
    estimate[j] <- mean(dnorm(x[j], values, bandwidth))
  }
  lines(x, estimate, col = colors[i], lwd = 3)
}
text(c(0.5, 1.9, 3.3), c(2.2, 2.2, 2.2),
     c("n = 1", "n = 5", "n = 30"), col = colors, cex = 1.1)`,
  },
  {
    id: "distribution",
    title: "Meet the bell curve",
    category: "Statistics",
    description: "Generate a population and see how randomness takes shape.",
    mode: "plot",
    color: "#5c817b",
    code: `# A thousand tiny surprises
set.seed(7)
samples <- rnorm(1000)
hist(samples, breaks = 24, col = "#92b4a7",
     border = "white", main = "Beautifully ordinary",
     xlab = "Value", ylab = "Frequency")`,
  },
  {
    id: "waves",
    title: "Separate the seasons",
    category: "Statistics",
    description:
      "A time series hides a trend inside its seasonal rhythm. Compare the signal with what you observe.",
    mode: "plot",
    color: "#43877b",
    code: `# Separate a trend, a seasonal cycle, and measurement noise
set.seed(15)
month <- 1:72
trend <- 30 + month * 0.4
season <- 7 * sin(2 * pi * month / 12)
observed <- trend + season + rnorm(72, sd = 1.4)
plot(month, observed, type = "l", col = "#90afa5", lwd = 2,
     xlab = "Month", ylab = "Demand", main = "A rhythm beneath the noise")
lines(month, trend + season, col = "#39796c", lwd = 3)
lines(month, trend, col = "#c67d58", lwd = 3)
text(18, 57, "Seasonal signal", col = "#39796c")
text(18, 53, "Underlying trend", col = "#c67d58")`,
  },
  {
    id: "grid",
    title: "Find your rhythm",
    category: "Graphics",
    description:
      "Turn twelve weeks of small moments into a patterned calendar.",
    mode: "plot",
    color: "#24776a",
    code: `library(grid)
grid.newpage()
grid.rect(gp = gpar(fill = "#f8f6ef", col = NA))
grid.text("SMALL MOMENTS, OVER TIME", x = .12, y = .86,
          just = "left", gp = gpar(fontsize = 10, col = "#52695e"))
grid.text("Find your rhythm", x = .12, y = .77,
          just = "left", gp = gpar(fontsize = 28, col = "#244e44"))
colors <- c("#e1e7da", "#b8cec0", "#80af9e", "#488c79", "#1e6657")
for (week in 1:12) {
  for (day in 1:7) {
    level <- 1 + ((week * 3 + day * 7 + week * day) %% 5)
    grid.rect(x = .15 + (week - 1) * .064,
              y = .61 - (day - 1) * .054,
              width = .053, height = .043,
              gp = gpar(fill = colors[level], col = NA))
  }
}
grid.text(c("WEEK 01", "04", "08", "12"),
          x = c(.15, .342, .598, .854), y = .21,
          gp = gpar(fontsize = 9, col = "#52695e"))
grid.text("Twelve weeks. Eighty-four little possibilities.",
          x = .12, y = .11, just = "left",
          gp = gpar(fontsize = 11, col = "#52695e"))`,
  },
  {
    id: "plotmath",
    title: "See uncertainty take shape",
    category: "Statistics",
    description:
      "Connect a probability formula to its curve. Shade one standard deviation and see how much it contains.",
    mode: "plot",
    color: "#43877b",
    code: `# The formula, the curve, and the probability between
mu <- 0
sigma <- 1
x <- seq(-4, 4, length.out = 300)
y <- dnorm(x, mean = mu, sd = sigma)
plot(x, y, type = "n", ylim = c(0, 0.62),
     xlab = "Standard deviations from the mean", ylab = "Density",
     main = "The shape of uncertainty")
inside <- seq(-sigma, sigma, length.out = 100) + mu
polygon(c(inside[1], inside, inside[length(inside)]),
        c(0, dnorm(inside, mu, sigma), 0),
        col = "#93bfae", border = NA)
lines(x, y, col = "#43877b", lwd = 3)
text(0, 0.52,
     expression(f(x) == frac(1, sigma * sqrt(2 * pi)) *
                e^(-frac((x - mu)^2, 2 * sigma^2))),
     cex = 1.3, col = "#bc8060")
probability <- pnorm(mu + sigma, mu, sigma) -
               pnorm(mu - sigma, mu, sigma)
text(0, 0.13, paste(round(100 * probability, 1), "%"),
     cex = 1.6, col = "#244e44")`,
  },
  {
    id: "boxplot",
    title: "Spot the outlier",
    category: "Statistics",
    description:
      "Compare the spread of three groups, with their quirks intact.",
    mode: "plot",
    color: "#719087",
    code: `set.seed(12)
a <- rnorm(40, 4, 1)
b <- rnorm(40, 6, 1.5)
c <- c(rnorm(38, 5, .7), 9, 10)
boxplot(list(A = a, B = b, C = c),
        col = "#b7c9bd", main = "Different by nature",
        ylab = "Observation")`,
  },
  {
    id: "bars",
    title: "Make the comparison",
    category: "Graphics",
    description: "Turn a simple vector into something that tells a story.",
    mode: "plot",
    color: "#d2874f",
    code: `hours <- c(Reading = 4, Making = 9, Exploring = 6, Resting = 7)
barplot(hours, col = "#c87951", border = NA,
        main = "A week well spent", ylab = "Hours")`,
  },
  {
    id: "spiral",
    title: "Estimate pi by chance",
    category: "Simulation",
    description:
      "Random points estimate an ancient constant. More samples bring the estimate closer to pi.",
    mode: "plot",
    color: "#43877b",
    code: `# Monte Carlo integration: area estimates a constant
set.seed(27)
x <- runif(1600, -1, 1)
y <- runif(1600, -1, 1)
inside <- x^2 + y^2 <= 1
colors <- ifelse(inside, "#589688", "#d6b59b")
plot(x, y, pch = 16, cex = 0.45, col = colors,
     asp = 1, xlab = "x", ylab = "y",
     main = paste("An estimate of pi:", round(4 * mean(inside), 3)))
t <- seq(0, 2 * pi, length.out = 300)
lines(cos(t), sin(t), col = "#31665d", lwd = 2)`,
  },
  {
    id: "matrix",
    title: "Let R do the algebra",
    category: "Everyday R",
    description:
      "Solve a linear system using the faer-backed numerical runtime.",
    mode: "console",
    color: "#577c73",
    code: `A <- matrix(c(4, 1, 2, 1, 3, 0, 2, 0, 5), nrow = 3)
b <- c(7, 4, 9)
x <- solve(A, b)
cat("Solution:\\n")
print(round(x, 4))
cat("Check A %*% x:\\n")
print(A %*% x)`,
  },
  {
    id: "data",
    title: "A little data wrangling",
    category: "Everyday R",
    description: "Build a data frame, select rows, and calculate a summary.",
    mode: "console",
    color: "#8c735c",
    code: `plants <- data.frame(
  name = c("Monstera", "Pothos", "Fern", "Ficus"),
  height = c(80, 35, 42, 110),
  light = c("medium", "low", "low", "bright")
)
print(plants[plants$height > 40, ])
cat("Average height:", mean(plants$height), "cm")`,
  },
  {
    id: "simulation",
    title: "Roll with it",
    category: "Everyday R",
    description:
      "Run ten thousand dice rolls without writing a JavaScript simulation.",
    mode: "console",
    color: "#847597",
    code: `set.seed(123)
rolls <- sample(1:6, size = 10000, replace = TRUE)
cat("Observed probabilities:\\n")
print(round(table(rolls) / length(rolls), 3))
cat("Average roll:", round(mean(rolls), 3))`,
  },
  {
    id: "random-walk",
    title: "See a random walk",
    category: "Statistics",
    description:
      "Let uniform steps wander, then watch chance leave a shape behind.",
    mode: "plot",
    color: "#4e7c78",
    code: `set.seed(24)
steps <- sample(c(-1, 1), 240, replace = TRUE)
walk <- cumsum(steps)
plot(seq_along(walk), walk, type = "l", lwd = 2,
     col = "#3f766c", xlab = "Step", ylab = "Position",
     main = "A path made by chance")
abline(h = 0, col = "#c8d4cb", lty = 2)`,
  },
  {
    id: "orbit-lines",
    title: "Measure an area by chance",
    category: "Simulation",
    description: "Random darts estimate the area under a curve. Count the hits, then change the function.",
    mode: "plot",
    color: "#538e7d",
    code: `# Monte Carlo integration under a Gaussian-shaped curve
set.seed(51)
x <- runif(1800, 0, 2)
y <- runif(1800, 0, 1)
f <- function(x) exp(0 - x * x)
hit <- y <= f(x)
area <- 2 * mean(hit)
plot(x, y, pch = 16, cex = 0.4,
     col = ifelse(hit, "#4c927e", "#d3b6a0"),
     main = paste("Estimated area:", round(area, 3)),
     xlab = "x", ylab = "exp(-x squared)")
curve_x <- seq(0, 2, length.out = 200)
lines(curve_x, f(curve_x), col = "#285e51", lwd = 3)`,
  },
  {
    id: "constellation",
    title: "Explore possible futures",
    category: "Simulation",
    description:
      "One process can take many paths. Simulate uncertain growth and see how the possibilities spread.",
    mode: "plot",
    color: "#43877b",
    code: `# Independent random walks share a trend, not a destination
set.seed(19)
time <- 0:80
plot(time, time, type = "n", ylim = c(-20, 65),
     xlab = "Time", ylab = "Change", main = "One process, many possible futures")
colors <- c("#b9ccc1", "#93b6a8", "#70a18f", "#548674", "#35695b")
for (i in 1:70) {
  path <- c(0, cumsum(rnorm(80, mean = 0.35, sd = 1.8)))
  lines(time, path, col = colors[1 + (i %% 5)], lwd = 0.7)
}
lines(time, time * 0.35, col = "#d58c62", lwd = 3)
text(18, 55, "70 simulated paths", col = "#548674")
text(18, 49, "Expected trend", col = "#d58c62")`,
  },
  {
    id: "dice-counts",
    title: "Drop a needle, discover pi",
    category: "Simulation",
    description: "Buffon’s experiment turns random angles and parallel lines into another estimate of pi.",
    mode: "plot",
    color: "#538e7d",
    code: `# Buffon's needle: length 0.75, parallel lines one unit apart
set.seed(81)
n <- 240
length <- 0.75
x <- runif(n, 0.5, 5.5)
y <- runif(n, 0, 4)
angle <- runif(n, 0, pi)
dx <- length * cos(angle) / 2
dy <- length * sin(angle) / 2
crosses <- floor(y - dy) != floor(y + dy)
estimate <- 2 * length / mean(crosses)
plot.new()
plot.window(xlim = c(0, 6), ylim = c(-0.5, 4.5))
for (i in 0:4) abline(h = i, col = "#bccbc1", lwd = 1)
segments(x - dx, y - dy, x + dx, y + dy,
         col = ifelse(crosses, "#c77f60", "#4c8877"), lwd = 2)
title(main = paste("Needles estimate pi:", round(estimate, 3)))`,
  },
  {
    id: "sunset-ridges",
    title: "Give an estimate room to breathe",
    category: "Simulation",
    description:
      "Resample observed data to estimate uncertainty. A bootstrap distribution makes the interval visible.",
    mode: "plot",
    color: "#43877b",
    code: `# Bootstrap the mean from an observed sample
set.seed(8)
observed <- c(18, 22, 25, 19, 31, 27, 24, 21, 35, 29, 23, 26)
boot <- numeric(800)
for (i in 1:800) {
  boot[i] <- mean(sample(observed, length(observed), replace = TRUE))
}
hist(boot, breaks = 28, col = "#8db6a5", border = "white",
     main = "How certain is this average?", xlab = "Resampled mean", ylab = "Frequency")
# Interpolate the 2.5th and 97.5th percentiles (R type 7)
ordered <- sort(boot)
ranks <- 1 + (length(boot) - 1) * c(0.025, 0.975)
low <- floor(ranks)
interval <- ordered[low] + (ranks - low) * (ordered[low + 1] - ordered[low])
abline(v = interval, col = "#c77f58", lwd = 3, lty = 2)
abline(v = mean(observed), col = "#305e53", lwd = 3)
cat("Observed mean:", mean(observed), "\\n")
cat("95% bootstrap interval:", round(interval, 2))`,
  },
  {
    id: "woven-rosette",
    title: "Find hidden relationships",
    category: "Graphics",
    description:
      "A correlation matrix reveals which measurements move together, in opposite directions, or independently.",
    mode: "plot",
    color: "#57998d",
    code: `# Turn correlations into a matrix you can read at a glance
library(grid)
set.seed(33)
sun <- rnorm(160)
water <- rnorm(160)
growth <- 0.8 * sun + 0.5 * water + rnorm(160, sd = 0.4)
stress <- -0.7 * water + rnorm(160, sd = 0.5)
data <- cbind(sun, water, growth, stress)
r <- cor(data)
labels <- c("Sun", "Water", "Growth", "Stress")
colors <- c("#914e5d", "#b8757d", "#d6a4a5", "#ead2c7", "#f1ecdf",
            "#c9ddd0", "#94beab", "#548f7c", "#286452")
grid.newpage()
grid.rect(gp = gpar(fill = "#f8f6ef", col = NA))
grid.text("What moves together?", x = .5, y = .92,
          gp = gpar(fontsize = 24, col = "#244e44"))
for (i in 1:4) {
  for (j in 1:4) {
    value <- r[i, j]
    grid.rect(x = .29 + (j - 1) * .16, y = .72 - (i - 1) * .16,
              width = .15, height = .15,
              gp = gpar(fill = colors[1 + round((value + 1) * 4)], col = NA))
    grid.text(round(value, 2), x = .29 + (j - 1) * .16,
              y = .72 - (i - 1) * .16,
              gp = gpar(fontsize = 17,
                        col = ifelse(abs(value) > .6, "#ffffff", "#244e44")))
  }
}
grid.text(labels, x = seq(.29, .77, length.out = 4), y = .12,
          gp = gpar(fontsize = 13, col = "#52695e"))
grid.text(labels, x = .17, y = seq(.72, .24, length.out = 4),
          just = "right", gp = gpar(fontsize = 13, col = "#52695e"))`,
  },
]
export const categories = [
  "All examples",
  "Statistics",
  "Graphics",
  "Simulation",
  "Everyday R",
]
