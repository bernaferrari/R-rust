// Validate the shipped artifact, not merely the linker command that built it.
export const MAX_WASM_PAGES = 4096 // 256 MiB in 64 KiB Wasm pages.
export function assertBoundedMemory(bytes) {
  const header = [0, 97, 115, 109, 1, 0, 0, 0]
  if (header.some((byte, i) => bytes[i] !== byte))
    throw new Error("Invalid Wasm header")
  let offset = 8
  function uint(end = bytes.length) {
    let value = 0
    for (let shift = 0; shift <= 28; shift += 7) {
      if (offset >= end) throw new Error("Truncated Wasm integer")
      const byte = bytes[offset++]
      value += (byte & 127) * 2 ** shift
      if (value > 0xffffffff) throw new Error("Invalid Wasm integer")
      if (!(byte & 128)) return value
    }
    throw new Error("Invalid Wasm integer")
  }
  let found = false
  while (offset < bytes.length) {
    const id = bytes[offset++]
    const length = uint()
    const end = offset + length
    if (end > bytes.length) throw new Error("Truncated Wasm section")
    if (id === 5) {
      if (found || uint(end) !== 1 || uint(end) !== 1)
        throw new Error("Expected one bounded, unshared 32-bit Wasm memory")
      const minimum = uint(end)
      const maximum = uint(end)
      if (minimum > maximum || maximum > MAX_WASM_PAGES || offset !== end)
        throw new Error("Wasm memory must have a maximum of 256 MiB or less")
      found = true
    }
    offset = end
  }
  if (!found)
    throw new Error(
      "Wasm runtime has no declared memory ceiling; rebuild with pnpm build:runtime"
    )
}
