analyze_sales <- function() {
    files <- tempfile(c("sales-", "stores-", "summary-"), fileext = ".csv")
    on.exit(unlink(files))
    writeLines(c("id,store,units,price,product",
                 '1,A,2,12.5,"Tea, green"',
                 "2,B,4,5.25,Coffee",
                 "3,A,NA,4,Tea",
                 "4,C,3,9.5,Cocoa",
                 "5,B,2,7,Coffee",
                 "6,A,5,4,Tea"), files[1])
    writeLines(c("store,region", "A,North", "B,North", "C,South"), files[2])
    sales <- read.csv(files[1], colClasses = c("integer", "character", "integer", "numeric", "character"))
    stores <- read.csv(files[2], colClasses = "character")
    stopifnot(sales$product[1] == "Tea, green", sum(is.na(sales$units)) == 1L)
    sales <- merge(sales, stores, by = "store", all.x = TRUE)
    sales$revenue <- with(sales, units * price)
    summary <- aggregate(revenue ~ region, sales, sum)
    stopifnot(identical(summary$region, c("North", "South")),
              identical(summary$revenue, c(80, 28.5)))
    write.csv(summary, files[3], row.names = FALSE)
    stopifnot(identical(read.csv(files[3]), summary))
}

analyze_sales()
cat("CSV import, joins, grouping and export agree\n")
