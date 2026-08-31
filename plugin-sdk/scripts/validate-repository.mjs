import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import Ajv2020 from 'ajv/dist/2020.js'
import addFormats from 'ajv-formats'
import {
  compareSemver,
  discoverOfficialPlugins,
  hasFlag,
  officialHomepage,
  parseCargoPackage,
  readJSON,
  releaseURLs,
  repositoryRoot,
} from './repository-utils.mjs'

const maximumManifestBytes = 1024 * 1024
const maximumPackageBytes = 64 * 1024 * 1024
const registry = await readJSON(resolve(repositoryRoot, 'ohmycine-plugin-registry.v1.json'))
const registrySchema = await readJSON(resolve(repositoryRoot, 'plugin-sdk/schema/registry-v1.schema.json'))
const manifestSchema = await readJSON(resolve(repositoryRoot, 'plugin-sdk/schema/manifest-v1.schema.json'))
const ajv = new Ajv2020({ allErrors: true, strict: true })
addFormats(ajv)
const validateRegistry = ajv.compile(registrySchema)
const validateManifest = ajv.compile(manifestSchema)

if (!validateRegistry(registry))
  throw new Error(`official registry is invalid: ${ajv.errorsText(validateRegistry.errors)}`)
if (registry.repository.homepage !== officialHomepage)
  throw new Error(`registry homepage must be ${officialHomepage}`)

const plugins = new Map()
for (const pluginName of await discoverOfficialPlugins()) {
  const pluginRoot = resolve(repositoryRoot, 'plugins/official', pluginName)
  const manifest = await readJSON(resolve(pluginRoot, 'plugin.template.json'))
  const release = await readJSON(resolve(pluginRoot, 'release.json'))
  const cargo = parseCargoPackage(await readFile(resolve(pluginRoot, 'Cargo.toml'), 'utf8'))
  const materialized = { ...manifest, packageSha256: '0'.repeat(64) }
  if (!validateManifest(materialized))
    throw new Error(`${pluginName} manifest template is invalid: ${ajv.errorsText(validateManifest.errors)}`)
  if (manifest.packageSha256 !== '${PACKAGE_SHA256}')
    throw new Error(`${pluginName} manifest must keep the package digest placeholder`)
  if (manifest.source !== officialHomepage)
    throw new Error(`${pluginName} manifest source must be ${officialHomepage}`)
  if (cargo.version !== manifest.version)
    throw new Error(`${pluginName} Cargo and manifest versions differ`)
  validateReleaseConfig(pluginName, release)
  if (plugins.has(manifest.id)) throw new Error(`duplicate official plugin id: ${manifest.id}`)
  plugins.set(manifest.id, { pluginName, manifest, cargo })
}

const seenRegistryIDs = new Set()
for (const entry of registry.plugins) {
  if (seenRegistryIDs.has(entry.id)) throw new Error(`duplicate registry plugin id: ${entry.id}`)
  seenRegistryIDs.add(entry.id)
  const source = plugins.get(entry.id)
  if (!source) throw new Error(`registry plugin has no official source directory: ${entry.id}`)
  if (compareSemver(entry.version, source.manifest.version) > 0)
    throw new Error(`registry version is newer than source for ${entry.id}`)
  const expected = releaseURLs(source.pluginName, entry.id, entry.version)
  if (entry.manifestUrl !== expected.manifestUrl || entry.packageUrl !== expected.packageUrl)
    throw new Error(`${entry.id} release URLs do not match tag ${expected.tag}`)
  if (hasFlag('online')) await validatePublishedAssets(entry)
}

console.log(`validated ${plugins.size} official plugin source(s) and ${registry.plugins.length} Registry entry/entries${hasFlag('online') ? ' with published assets' : ''}`)

function validateReleaseConfig(pluginName, release) {
  if (release?.schemaVersion !== 1 || !['stable', 'beta'].includes(release.channel))
    throw new Error(`${pluginName} release config has an invalid schema/channel`)
  if (!Array.isArray(release.categories) || release.categories.length === 0 || release.categories.length > 12
      || new Set(release.categories).size !== release.categories.length
      || release.categories.some(value => typeof value !== 'string' || !/^[a-z0-9.-]+$/.test(value)))
    throw new Error(`${pluginName} release categories are invalid`)
  if (typeof release.releaseNotes !== 'string' || release.releaseNotes.length > 4000)
    throw new Error(`${pluginName} release notes are invalid`)
}

async function validatePublishedAssets(entry) {
  const [manifestBytes, packageBytes] = await Promise.all([
    boundedDownload(entry.manifestUrl, maximumManifestBytes),
    boundedDownload(entry.packageUrl, maximumPackageBytes),
  ])
  const digest = createHash('sha256').update(packageBytes).digest('hex')
  if (digest !== entry.packageSha256) throw new Error(`${entry.id} published package SHA-256 differs from Registry`)
  const manifest = JSON.parse(manifestBytes.toString('utf8'))
  if (!validateManifest(manifest))
    throw new Error(`${entry.id} published manifest is invalid: ${ajv.errorsText(validateManifest.errors)}`)
  if (manifest.id !== entry.id || manifest.version !== entry.version || manifest.packageSha256 !== digest || manifest.source !== officialHomepage)
    throw new Error(`${entry.id} published manifest identity/digest/source differs from Registry`)
}

async function boundedDownload(url, maximumBytes) {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), 30000)
  try {
    const response = await fetch(url, { redirect: 'follow', signal: controller.signal, headers: { 'User-Agent': 'OhMyCine-Plugins-CI' } })
    if (!response.ok) throw new Error(`download failed with HTTP ${response.status}: ${url}`)
    const declared = Number(response.headers.get('content-length') || 0)
    if (declared > maximumBytes) throw new Error(`asset exceeds size limit: ${url}`)
    const bytes = Buffer.from(await response.arrayBuffer())
    if (bytes.length === 0 || bytes.length > maximumBytes) throw new Error(`asset has invalid size: ${url}`)
    return bytes
  } finally {
    clearTimeout(timeout)
  }
}
