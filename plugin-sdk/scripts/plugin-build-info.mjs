import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { option, parseCargoPackage, readJSON, repositoryRoot } from './repository-utils.mjs'

const pluginName = option('plugin')
if (!pluginName || !/^[a-z0-9-]+$/.test(pluginName)) throw new Error('--plugin must be a safe official plugin directory name')
const pluginRoot = resolve(repositoryRoot, 'plugins/official', pluginName)
const cargo = parseCargoPackage(await readFile(resolve(pluginRoot, 'Cargo.toml'), 'utf8'))
const manifest = await readJSON(resolve(pluginRoot, 'plugin.template.json'))
if (cargo.version !== manifest.version) throw new Error(`${pluginName} Cargo and manifest versions differ`)
process.stdout.write(JSON.stringify({ name: pluginName, version: manifest.version, pluginID: manifest.id, wasmStem: cargo.wasmStem }))
