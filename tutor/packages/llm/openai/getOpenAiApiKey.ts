export { getOpenAiApiKey }

function getOpenAiApiKey(action: string): string {
  const apiKey = process.env['OPENAI_API_KEY']

  if (!apiKey) {
    throw new Error(`Set OPENAI_API_KEY in packages/llm/.env before ${action}.`)
  }

  return apiKey
}
