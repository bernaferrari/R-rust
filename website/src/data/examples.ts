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
  if (example.id === "grid") {
    return example.code
      .replaceAll("#f8f6ef", "#171e1c")
      .replaceAll("#52695e", "#9fb4a8")
      .replaceAll("#244e44", "#d7e8de")
  }
  const code = example.code.replaceAll('border = "white"', 'border = "#30403b"')
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
    title: "Grow a spiral garden",
    category: "Creative",
    description:
      "Turn the golden angle into a garden of sculpted petals. Three blooms, one simple rule.",
    mode: "plot",
    color: "#43877b",
    code: `# A spiral garden: every petal turns by the golden angle
plot.new()
plot.window(xlim = c(-1.4, 1.4), ylim = c(-1, 1), asp = 1)
t <- seq(0, 2 * pi, length.out = 24)
golden <- pi * (3 - sqrt(5))

bloom <- function(cx, cy, size, colors) {
  for (k in 150:1) {
    angle <- k * golden
    r <- size * sqrt(k / 150)
    # Radial petals overlap like the scales of a pine cone
    a <- size * (0.10 + 0.045 * sqrt(k / 150))
    b <- a * 0.42
    u <- r + a * cos(t)
    v <- b * sin(t)
    polygon(cx + u * cos(angle) - v * sin(angle),
            cy + u * sin(angle) + v * cos(angle),
            col = colors[1 + (k %% length(colors))], border = NA)
  }
}
bloom(0.34, 0.12, 0.73,
      c("#24594f", "#357466", "#4b907c", "#7fb69a", "#b3d4b4"))
bloom(-0.80, 0.38, 0.32,
      c("#9e4635", "#bb6549", "#d58e65", "#edbd91"))
bloom(-0.66, -0.49, 0.24,
      c("#ae813b", "#caa05a", "#dfbd7e", "#eed6a7"))`,
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
    title: "Draw an orbit",
    category: "Graphics",
    description:
      "Two quiet frequencies make a looping figure with a compass-like rhythm.",
    mode: "plot",
    color: "#586d92",
    code: `t <- seq(0, 2 * pi, length.out = 500)
x <- 1.2 * sin(3 * t + pi / 2)
y <- 0.8 * sin(2 * t)
plot(x, y, type = "l", lwd = 3, col = "#586d92",
     axes = FALSE, xlab = "", ylab = "", asp = 1,
     main = "An orbit in two frequencies")
points(x[c(1, 126, 251, 376)], y[c(1, 126, 251, 376)],
       pch = 16, cex = 1.2, col = "#d2874f")`,
  },
  {
    id: "constellation",
    title: "Map a constellation",
    category: "Creative",
    description:
      "A handful of points becomes a little night sky when you connect the dots.",
    mode: "plot",
    color: "#4b5878",
    code: `set.seed(18)
stars <- matrix(runif(14, -1, 1), ncol = 2)
plot(stars, pch = 16, cex = 1.4, col = "#d8ad58",
     axes = FALSE, xlab = "", ylab = "", asp = 1,
     xlim = c(-1.1, 1.1), ylim = c(-1.1, 1.1),
     main = "A small constellation")
for (i in 1:6) {
  j <- i + 1
  segments(stars[i, 1], stars[i, 2], stars[j, 1], stars[j, 2],
           col = "#697995", lwd = 1.5)
}`,
  },
  {
    id: "dice-counts",
    title: "Count the dice",
    category: "Everyday R",
    description:
      "A compact simulation that turns ten thousand rolls into a readable check.",
    mode: "console",
    color: "#7d6a92",
    code: `set.seed(31)
rolls <- sample(1:6, 10000, replace = TRUE)
counts <- tabulate(rolls, nbins = 6)
cat("Roll counts:\n")
print(counts)
cat("Most common face:", which.max(counts), "\\n")
cat("Mean roll:", round(mean(rolls), 3))`,
  },
]
export const categories = [
  "All examples",
  "Statistics",
  "Graphics",
  "Creative",
  "Everyday R",
]
