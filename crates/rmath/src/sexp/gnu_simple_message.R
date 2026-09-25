function(message, call = NULL)
    structure(list(message = as.character(message), call = call),
              class = c("simpleMessage", "message", "condition"))
