export { runSourceTests, type RunSourceTestsOptions }

import { spawn } from 'node:child_process'
import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, relative, resolve } from 'node:path'

import { sourceTests } from '#testing/source-tests/sourceTests.ts'
import type { SourceTestContext } from '#testing/source-tests/sourceTests.ts'

async function runSourceTests(options: RunSourceTestsOptions = {}): Promise<number> {
  const runOptions = {
    ...(options.arguments ? parseRunSourceTestsArguments(options.arguments) : {}),
    ...options,
  }
  const files = await findSourceTestFiles({
    cwd: runOptions.cwd ?? process.cwd(),
    paths: runOptions.paths?.length ? runOptions.paths : ['.'],
  })

  return files.length
    ? await runNodeTests({
        cwd: runOptions.cwd ?? process.cwd(),
        files,
        nodeArguments: runOptions.nodeArguments ?? [],
      })
    : 0
}

function parseRunSourceTestsArguments(arguments_: readonly string[]): RunSourceTestsOptions {
  return arguments_.includes('--')
    ? {
        nodeArguments: arguments_.slice(arguments_.indexOf('--') + 1),
        paths: arguments_.slice(0, arguments_.indexOf('--')),
      }
    : { paths: arguments_ }
}

async function runNodeTests(
  options: Required<Pick<RunSourceTestsOptions, 'cwd' | 'nodeArguments'>> & { files: string[] },
): Promise<number> {
  return await new Promise(resolveTestRun => {
    const testRun = spawn(process.execPath, ['--test', ...options.nodeArguments, ...options.files], {
      cwd: options.cwd,
      stdio: 'inherit',
    })

    testRun.on('exit', (code, signal) => {
      resolveTestRun(signal ? 1 : (code ?? 1))
    })
  })
}

async function findSourceTestFiles(options: Required<Pick<RunSourceTestsOptions, 'cwd' | 'paths'>>): Promise<string[]> {
  return (await Promise.all(options.paths.map(async path => await findSourceTestFilesAtPath(resolve(options.cwd, path))))).flat().sort()
}

async function findSourceTestFilesAtPath(path: string): Promise<string[]> {
  const pathStat = await stat(path).catch(() => undefined)

  return pathStat?.isDirectory()
    ? isIgnoredDirectory(path)
      ? []
      : (
          await Promise.all(
            (await readdir(path, { withFileTypes: true })).map(async entry => await findSourceTestFilesAtPath(join(path, entry.name))),
          )
        ).flat()
    : pathStat?.isFile() && isTypeScriptSourceFile(path) && (await fileHasSourceTests(path))
      ? [path]
      : []
}

function isTypeScriptSourceFile(path: string): boolean {
  return path.endsWith('.ts') && !path.endsWith('.d.ts')
}

function isIgnoredDirectory(path: string): boolean {
  return ['.git', '.svelte-kit', 'build', 'coverage', 'dist', 'node_modules'].includes(path.split('/').at(-1) ?? '')
}

async function fileHasSourceTests(path: string): Promise<boolean> {
  return (await readFile(path, 'utf8')).includes('sourceTests(')
}

type RunSourceTestsOptions = {
  arguments?: readonly string[]
  cwd?: string
  nodeArguments?: readonly string[]
  paths?: readonly string[]
}

sourceTests(import.meta, (context: SourceTestContext) => {
  context.test('parses source test runner arguments', () => {
    context.assert.deepEqual(parseRunSourceTestsArguments(['src', '--', '--test-name-pattern', 'loads']), {
      nodeArguments: ['--test-name-pattern', 'loads'],
      paths: ['src'],
    })

    context.assert.deepEqual(parseRunSourceTestsArguments(['src', 'index.ts']), {
      paths: ['src', 'index.ts'],
    })

    context.assert.deepEqual(parseRunSourceTestsArguments([]), {
      paths: [],
    })
  })

  context.test('finds source files with source tests', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'ai-testing-'))

    try {
      await mkdir(join(directory, 'src'))
      await mkdir(join(directory, 'node_modules'))
      await writeFile(join(directory, 'src', 'withTests.ts'), 'sourceTests(() => {})')
      await writeFile(join(directory, 'src', 'withoutTests.ts'), 'export {}')
      await writeFile(join(directory, 'src', 'types.d.ts'), 'sourceTests(() => {})')
      await writeFile(join(directory, 'node_modules', 'ignored.ts'), 'sourceTests(() => {})')

      context.assert.deepEqual(
        (await findSourceTestFiles({ cwd: directory, paths: ['.'] })).map(path => relative(directory, path)),
        [join('src', 'withTests.ts')],
      )
    } finally {
      await rm(directory, { force: true, recursive: true })
    }
  })
})
