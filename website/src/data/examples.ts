export type Example = {
  id: string
  title: string
  category: string
  description: string
  mode: "plot" | "console"
  code: string
  color: string
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
    title: "Nature has a formula",
    category: "Creative",
    description:
      "Grow a sunflower from the golden angle. Just seven lines of R.",
    mode: "plot",
    color: "#bf773b",
    code: `# Phyllotaxis: the geometry of a sunflower
n <- 700
i <- seq_len(n)
angle <- i * pi * (3 - sqrt(5))
radius <- sqrt(i)
plot(radius * cos(angle), radius * sin(angle),
     pch = 16, cex = 0.65, col = "#bf773b",
     axes = FALSE, xlab = "", ylab = "")`,
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
    title: "Make some waves",
    category: "Creative",
    description: "Layer sine waves into a small piece of mathematical art.",
    mode: "plot",
    color: "#75659a",
    code: `x <- seq(0, 4 * pi, length.out = 250)
plot(x, sin(x), type = "n", ylim = c(-2, 5),
     axes = FALSE, xlab = "", ylab = "")
colors <- c("#3d5b56", "#67867e", "#a3b8a5",
            "#d7b878", "#cc774b", "#b14e38")
for (i in 1:6) {
  lines(x, sin(x + i / 2) + i * 0.55,
        col = colors[i], lwd = 3)
}`,
  },
  {
    id: "grid",
    title: "Think in layers",
    category: "Graphics",
    description:
      "Compose a graphic with grid viewports, shapes, and typography.",
    mode: "plot",
    color: "#cb664c",
    code: `library(grid)
grid.newpage()
pushViewport(viewport(width = 0.8, height = 0.8))
grid.rect(gp = gpar(fill = "#f0e9dc", col = NA))
grid.circle(x = .36, y = .52, r = .27,
            gp = gpar(fill = "#cc5636", col = NA))
grid.circle(x = .64, y = .52, r = .27,
            gp = gpar(fill = "#2c625a", col = NA))
grid.text("hello, grid.", y = .16,
          gp = gpar(fontsize = 28, col = "#252a27"))
popViewport()`,
  },
  {
    id: "plotmath",
    title: "Say it with symbols",
    category: "Graphics",
    description:
      "Fractions, Greek letters, radicals, and a little mathematical poetry.",
    mode: "plot",
    color: "#414c65",
    code: `plot.new()
plot.window(xlim = c(0, 1), ylim = c(0, 1))
text(.5, .7, expression(frac(alpha[1]^2, sqrt(beta))),
     cex = 3, col = "#345d55")
text(.5, .3, expression(sum(x[i], i == 1, n)),
     cex = 2, col = "#cc5636")`,
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
    title: "Follow your curiosity",
    category: "Creative",
    description:
      "A parametric spiral. Change the frequency and see what happens.",
    mode: "plot",
    color: "#ba634a",
    code: `t <- seq(0, 12 * pi, length.out = 1200)
x <- t * cos(t)
y <- t * sin(t)
plot(x, y, type = "l", col = "#bb644a", lwd = 2,
     axes = FALSE, xlab = "", ylab = "")`,
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
]
export const categories = [
  "All examples",
  "Statistics",
  "Graphics",
  "Creative",
  "Everyday R",
]
