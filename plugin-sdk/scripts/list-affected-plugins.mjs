import { execFileSync } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { discoverOfficialPlugins, option, parseCargoPackage, readJSON, repositoryRoot } from './repository-utils.mjs'

const all = await discoverOfficialPlugins()
const base = option('base')
const head = option('head', 'HEAD')
let selected = all
if (base && !/^0+$/.test(base)) {
  const changed = execFileSync('git', ['diff', '--name-only', `${base}..${head}`], { encoding: 'utf8' }).split(/\r?\n/).filter(Boolean)
  const sharedChanged = changed.some(path => path.startsWith('plugin-sdk/')
    || path === 'ohmycine-plugin-registry.v1.json'
    || path === 'plugins/ohmycine-plugin-registry.v1.template.json'
    || path.startsWith('.github/workflows/'))
  if (!sharedChanged) {
    const names = new Set(changed.map(path => /^plugins\/official\/([^/]+)\//.exec(path)?.[1]).filter(Boolean))
    selected = all.filter(name => names.has(name))
  }
}
const values = await Promise.all(selected.map(async (name) => {
  const pluginRoot = resolve(repositoryRoot, 'plugins/official', name)
  const cargo = parseCargoPackage(await readFile(resolve(pluginRoot, 'Cargo.toml'), 'utf8'))
  const manifest = await readJSON(resolve(pluginRoot, 'plugin.template.json'))
  if (cargo.version !== manifest.version) throw new Error(`${name} Cargo and manifest versions differ`)
  return { name, version: manifest.version, pluginID: manifest.id, wasmStem: cargo.wasmStem }
}))
process.stdout.write(JSON.stringify(values))
