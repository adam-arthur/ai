#!/usr/bin/env node

import { runSourceTests } from '#testing/source-tests/runSourceTests.ts'

process.exitCode = await runSourceTests({ arguments: process.argv.slice(2) })
