@file:kotlin.js.JsModule("webr")

import kotlin.js.JsAny
import kotlin.js.JsString
import kotlin.js.Promise

external class WebR : JsAny {
    constructor(options: JsAny)
    fun init(): Promise<JsAny>
    fun interrupt()
    fun evalRString(code: String): Promise<JsString>
    fun installPackages(packages: String): Promise<JsAny>
}
