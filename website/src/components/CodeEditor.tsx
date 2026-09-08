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
  EditorView.theme({
    "&": {
      fontSize: "14px",
      fontFamily: '"SFMono-Regular", Consolas, monospace',
    },
    ".cm-scroller": { overflow: "auto", lineHeight: "1.65" },
    ".cm-content": { padding: "12px 52px 12px 8px" },
    ".cm-gutters": { border: "none", paddingLeft: "8px" },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 8px 0 5px",
      fontVariantNumeric: "tabular-nums",
    },
    "&.cm-focused": { outline: "none" },
  }),
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
        height="440px"
        extensions={dark ? [...extensions, darkComments] : extensions}
        onChange={onChange}
        aria-label="R code editor"
        basicSetup={{
          foldGutter: false,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
          highlightSelectionMatches: false,
          autocompletion: false,
        }}
      />
    </div>
  )
}
