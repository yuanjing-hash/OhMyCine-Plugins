import { createHash } from 'node:crypto'
import { readFile, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { packPlugin } from './pack-plugin.mjs'
import { officialHomepage, option, parseCargoPackage, readJSON, releaseEntry, repositoryRoot, strictSemver } from './repository-utils.mjs'

const tag = option('tag')
const wasmPath = option('wasm')
const outputDirectory = resolve(option('out', 'dist/release'))
const match = /^plugin-([a-z0-9-]+)-v(.+)$/.exec(tag || '')
if (!match || !strictSemver.test(match[2]))
  throw new Error('tag must match plugin-<name>-v<strict-semver>')
if (!wasmPath) throw new Error('--wasm is required')

const [, pluginName, version] = match
const pluginRoot = resolve(repositoryRoot, 'plugins/official', pluginName)
const manifest = await readJSON(resolve(pluginRoot, 'plugin.template.json'))
const release = await readJSON(resolve(pluginRoot, 'release.json'))
const cargo = parseCargoPackage(await readFile(resolve(pluginRoot, 'Cargo.toml'), 'utf8'))
if (manifest.version !== version || cargo.version !== version)
  throw new Error(`tag version ${version} differs from manifest/Cargo version`)
if (manifest.source !== officialHomepage)
  throw new Error(`manifest source must be ${officialHomepage}`)

const packed = await packPlugin({
  manifestPath: resolve(pluginRoot, 'plugin.template.json'),
  wasmPath: resolve(wasmPath),
  outputDirectory,
})
const { entry, tag: expectedTag, manifestName, packageName, checksumName } = releaseEntry({ pluginName, manifest, release, digest: packed.digest })
if (expectedTag !== tag) throw new Error('derived release tag differs from requested tag')
const checksum = `${packed.digest}  ${packageName}\n`
await writeFile(resolve(outputDirectory, checksumName), checksum, { mode: 0o600 })
const metadata = {
  schemaVersion: 1,
  tag,
  pluginName,
  pluginID: manifest.id,
  version,
  manifestName,
  packageName,
  checksumName,
  packageSha256: packed.digest,
  registryEntry: entry,
}
await writeFile(resolve(outputDirectory, 'release-metadata.json'), `${JSON.stringify(metadata, null, 2)}\n`, { mode: 0o600 })

const packageBytes = await readFile(resolve(outputDirectory, packageName))
if (createHash('sha256').update(packageBytes).digest('hex') !== packed.digest)
  throw new Error('package changed after it was written')
console.log(JSON.stringify(metadata, null, 2))
