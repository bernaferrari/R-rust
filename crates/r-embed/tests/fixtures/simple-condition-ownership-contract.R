x<-rawToChar(as.raw(c(97,255)));Encoding(x)<-"UTF-8"
y<-rawToChar(as.raw(c(98,128)));Encoding(y)<-"latin1"
m<-c(x,y);names(m)<-c("a","b")
for (name in c("simpleError","simpleWarning","simpleCondition")) {
 cnd<-get(name)(m,quote(target(x)))
 stopifnot(identical(names(cnd),c("message","call")),identical(cnd$call,quote(target(x))),identical(cnd$message,unname(m)),is.null(names(cnd$message)))
 stopifnot(identical(Encoding(cnd$message),c("UTF-8","latin1")),identical(charToRaw(cnd$message[1L]),as.raw(c(97,255))),identical(charToRaw(cnd$message[2L]),as.raw(c(98,128))))
 stopifnot(identical(get(name)(NULL)$message,character()),identical(get(name)(c(NA_character_,"ok"))$message,c(NA_character_,"ok")))
}
stopifnot(identical(simpleMessage(m,quote(target(x)))$message,m),identical(simpleMessage(1L)$message,1L),identical(simpleMessage(NULL)$message,NULL))
as.character.owned_condition_probe<-function(x,...) {gc();m}
z<-structure(17L,class="owned_condition_probe")
for (name in c("simpleError","simpleWarning","simpleCondition")) stopifnot(identical(get(name)(z,quote(target(x)))$message,m))
as.character.owned_condition_probe<-function(x,...) stop("unexpected coercion")
stopifnot(identical(simpleMessage(z)$message,z))
rm(as.character.owned_condition_probe)
stopifnot(identical(class(simpleError("x")),c("simpleError","error","condition")),identical(class(simpleWarning("x")),c("simpleWarning","warning","condition")),identical(class(simpleCondition("x")),c("simpleCondition","condition")),identical(class(simpleMessage("x")),c("simpleMessage","message","condition")))
TRUE
