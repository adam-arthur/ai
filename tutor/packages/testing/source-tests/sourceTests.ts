export { sourceTests, type SourceTestContext, type SourceTestsDefinition }

import type assert from 'node:assert/strict'
import type { after, afterEach, before, beforeEach, describe, it, mock, test } from 'node:test'

function sourceTests(module: ImportMeta, defineTests: SourceTestsDefinition): void {
  if (isSourceTestRunner(getProcessEnvironment()) && isSourceTestModule(module, getProcessArguments())) {
    void loadSourceTestContext().then(defineTests)
  }
}

async function loadSourceTestContext(): Promise<SourceTestContext> {
  const [assertModule, testModule] = await Promise.all([import('node:assert/strict'), import('node:test')])

  return {
    after: testModule.after,
    afterEach: testModule.afterEach,
    assert: assertModule.default,
    before: testModule.before,
    beforeEach: testModule.beforeEach,
    describe: testModule.describe,
    it: testModule.it,
    mock: testModule.mock,
    test: testModule.test,
  }
}

function isSourceTestRunner(environment: NodeJS.ProcessEnv): boolean {
  return environment['NODE_TEST_CONTEXT'] !== undefined
}

function isSourceTestModule(module: Pick<ImportMeta, 'filename'>, argv: readonly string[]): boolean {
  return module.filename === argv[1]
}

function getProcessEnvironment(): NodeJS.ProcessEnv {
  return typeof process === 'undefined' ? {} : process.env
}

function getProcessArguments(): readonly string[] {
  return typeof process === 'undefined' ? [] : process.argv
}

type SourceTestsDefinition = (context: SourceTestContext) => void

type SourceTestContext = {
  after: typeof after
  afterEach: typeof afterEach
  assert: typeof assert
  before: typeof before
  beforeEach: typeof beforeEach
  describe: typeof describe
  it: typeof it
  mock: typeof mock
  test: typeof test
}

sourceTests(import.meta, (context: SourceTestContext) => {
  context.test('detects node native test runner context', () => {
    context.assert.equal(isSourceTestRunner({ NODE_TEST_CONTEXT: 'child-v8' }), true)
    context.assert.equal(isSourceTestRunner({ NODE_TEST_CONTEXT: '' }), true)
    context.assert.equal(isSourceTestRunner({}), false)
  })

  context.test('detects the source test entry module', () => {
    context.assert.equal(isSourceTestModule({ filename: '/project/source.ts' }, ['node', '/project/source.ts']), true)
    context.assert.equal(isSourceTestModule({ filename: '/project/dependency.ts' }, ['node', '/project/source.ts']), false)
  })

  context.test('loads node native test helpers', async () => {
    context.assert.equal(typeof (await loadSourceTestContext()).assert.equal, 'function')
    context.assert.equal(typeof (await loadSourceTestContext()).test, 'function')
  })
})
