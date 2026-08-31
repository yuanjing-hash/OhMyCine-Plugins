import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import Ajv2020 from 'ajv/dist/2020.js'
import addFormats from 'ajv-formats'
import { unzipSync } from 'fflate'
import { officialHomepage, option, readJSON, releaseURLs, repositoryRoot } from './repository-utils.mjs'

const artifacts = resolve(option('artifacts', 'dist/release'))
const metadata = await readJSON(option('metadata', resolve(artifacts, 'release-metadata.json')))
const manifest = await readJSON(resolve(artifacts, metadata.manifestName))
const packageBytes = new Uint8Array(await readFile(resolve(artifacts, metadata.packageName)))
const checksum = (await readFile(resolve(artifacts, metadata.checksumName), 'utf8')).trim()
const digest = createHash('sha256').update(packageBytes).digest('hex')
const schema = await readJSON(resolve(repositoryRoot, 'plugin-sdk/schema/manifest-v1.schema.json'))
const ajv = new Ajv2020({ allErrors: true, strict: true })
addFormats(ajv)
const validate = ajv.compile(schema)

if (!validate(manifest)) throw new Error(`release manifest is invalid: ${ajv.errorsText(validate.errors)}`)
if (digest !== metadata.packageSha256 || manifest.packageSha256 !== digest)
  throw new Error('release package digest does not match metadata/manifest')
if (checksum !== `${digest}  ${metadata.packageName}`)
  throw new Error('release checksum file is invalid')
if (manifest.id !== metadata.pluginID || manifest.version !== metadata.version || manifest.source !== officialHomepage)
  throw new Error('release manifest identity/version/source is invalid')
const urls = releaseURLs(metadata.pluginName, metadata.pluginID, metadata.version)
if (metadata.tag !== urls.tag || metadata.registryEntry.manifestUrl !== urls.manifestUrl || metadata.registryEntry.packageUrl !== urls.packageUrl)
  throw new Error('release metadata URLs/tag are invalid')

const archive = unzipSync(packageBytes)
const expectedEntries = [manifest.entry, ...(manifest.libraryArtwork ? [manifest.libraryArtwork] : [])].sort()
const actualEntries = Object.keys(archive).sort()
if (JSON.stringify(actualEntries) !== JSON.stringify(expectedEntries))
  throw new Error(`package entries differ from manifest: ${actualEntries.join(', ')}`)
await WebAssembly.compile(archive[manifest.entry])
console.log(`verified ${metadata.tag} release artifacts (${digest})`)
