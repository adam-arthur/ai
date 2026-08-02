export { getGoogleApiKey }

function getGoogleApiKey(args: GoogleApiKeyRequest): string {
  const apiKey = process.env['GEMINI_API_KEY']

  if (!apiKey) {
    throw new Error(`Set GEMINI_API_KEY in packages/llm/.env before ${args.action}.`)
  }

  return apiKey
}

type GoogleApiKeyRequest = {
  action: string
}
