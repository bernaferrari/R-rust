import CodeMirror, { EditorView } from "@uiw/react-codemirror"
import { StreamLanguage } from "@codemirror/language"
import { r } from "@codemirror/legacy-modes/mode/r"
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
        height="390px"
        extensions={extensions}
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
