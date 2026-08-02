export { decodeBase64, encodeBase64, toError }

function encodeBase64(bytes: Uint8Array): string {
  let binary = ''

  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }

  return btoa(binary)
}

function decodeBase64(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), character => character.charCodeAt(0))
}

function toError(error: unknown): Error {
  if (error instanceof Error) {
    return error
  }

  return new Error(String(error))
}
