import CodeMirror, { EditorView } from "@uiw/react-codemirror"
import { tags } from "@lezer/highlight"
import {
  HighlightStyle,
  syntaxHighlighting,
  StreamLanguage,
} from "@codemirror/language"
import { r } from "@codemirror/legacy-modes/mode/r"
import { useDarkTheme } from "@/theme"
const darkComments = syntaxHighlighting(
  HighlightStyle.define([{ tag: tags.comment, class: "r-code-comment" }])
)
const extensions = [
  StreamLanguage.define(r),
  EditorView.contentAttributes.of({
    "aria-label": "R code editor",
    tabindex: "0",
  }),
]
export default function CodeEditor({
  code,
  onChange,
  onRun,
}: {
  code: string
  onChange: (code: string) => void
  onRun: () => void
}) {
  const dark = useDarkTheme()
  return (
    <div
      className="code-editor"
      onKeyDown={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
          event.preventDefault()
          onRun()
        }
      }}
    >
      <CodeMirror
        value={code}
        theme={dark ? "dark" : "light"}
        height="390px"
        extensions={dark ? [...extensions, darkComments] : extensions}
        onChange={onChange}
        aria-label="R code editor"
        basicSetup={{
          foldGutter: false,
          highlightActiveLine: false,
          highlightSelectionMatches: false,
          autocompletion: false,
        }}
      />
    </div>
  )
}
