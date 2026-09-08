export type ConsoleCommand = { code: string }
export type ConsoleExample = {
  title: string
  description: string
  commands: ConsoleCommand[]
  next: ConsoleCommand
}
export const consoleExamples: ConsoleExample[] = [
  {
    title: "A week in numbers",
    description: "Meet a dataset, find its average, then look closer.",
    commands: [
      {
        code: "temperatures <- c(19, 22, 24, 21, 18)\ntemperatures",
      },
      { code: "mean(temperatures)" },
      {
        code: 'plot(temperatures, type = "b", pch = 19, col = "#16766c",\n     main = "A week of small changes", xlab = "Day", ylab = "Temperature")',
      },
    ],
    next: {
      code: "temperatures[temperatures > mean(temperatures)]",
    },
  },
  {
    title: "Find the rhythm",
    description: "Build a wave with numbers. Give it a different beat.",
    commands: [
      {
        code: "x <- seq(0, 2 * pi, length.out = 120)\ny <- sin(x)\nrange(y)",
      },
      {
        code: 'plot(x, y, type = "l", lwd = 3, col = "#16766c",\n     main = "A little rhythm", xlab = "Time", ylab = "Signal")',
      },
    ],
    next: {
      code: 'plot(x, sin(2 * x), type = "l", lwd = 3, col = "#c55e43",\n     main = "Twice the rhythm", xlab = "Time", ylab = "Signal")',
    },
  },
  {
    title: "Let chance speak",
    description: "Roll a die, count the outcomes, and roll again.",
    commands: [
      {
        code: "set.seed(42)\nrolls <- sample(1:6, 1000, replace = TRUE)\nhead(rolls)",
      },
      { code: "table(rolls)" },
      {
        code: 'barplot(table(rolls), col = "#16766c", border = NA,\n        main = "A thousand little chances", xlab = "Face", ylab = "Rolls")',
      },
    ],
    next: { code: "mean(rolls)\nmean(rolls == 6)" },
  },
]
