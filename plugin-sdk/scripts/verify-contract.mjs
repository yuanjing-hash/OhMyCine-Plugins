import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import Ajv2020 from 'ajv/dist/2020.js'
import addFormats from 'ajv-formats'

const root = fileURLToPath(new URL('..', import.meta.url))
const schema = JSON.parse(await readFile(new URL('../schema/manifest-v1.schema.json', import.meta.url), 'utf8'))
const fixture = JSON.parse(await readFile(new URL('../fixtures/static-site/plugin.json', import.meta.url), 'utf8'))
const registrySchema = JSON.parse(await readFile(new URL('../schema/registry-v1.schema.json', import.meta.url), 'utf8'))
const registryFixture = JSON.parse(await readFile(new URL('../fixtures/repository/ohmycine-plugin-registry.v1.json', import.meta.url), 'utf8'))
const officialRegistry = JSON.parse(await readFile(new URL('../../ohmycine-plugin-registry.v1.json', import.meta.url), 'utf8'))
const onlineMediaFixture = JSON.parse(await readFile(new URL('../fixtures/online-media.v1.json', import.meta.url), 'utf8'))
const runtimeContract = await readFile(new URL('../src/runtime.ts', import.meta.url), 'utf8')
const offlineSchema = JSON.parse(await readFile(new URL('../schema/offline-download-plan-v1.schema.json', import.meta.url), 'utf8'))
const offlineFixture = JSON.parse(await readFile(new URL('../fixtures/offline-download-plan.v1.json', import.meta.url), 'utf8'))
const errorSchema = JSON.parse(await readFile(new URL('../schema/plugin-error-v1.schema.json', import.meta.url), 'utf8'))
const errorFixture = JSON.parse(await readFile(new URL('../fixtures/plugin-error.v1.json', import.meta.url), 'utf8'))
const selectionSchema = JSON.parse(await readFile(new URL('../schema/media-work-selection-v1.schema.json', import.meta.url), 'utf8'))
const ajv = new Ajv2020({ allErrors: true, strict: true })
addFormats(ajv)
const validate = ajv.compile(schema)
const validateRegistry = ajv.compile(registrySchema)
const validateOffline = ajv.compile(offlineSchema)
const validateError = ajv.compile(errorSchema)
const validateSelection = ajv.compile(selectionSchema)
function validWorkSelection(work) {
  return validateSelection(work) && (work.defaultSegmentId === undefined
    || work.segments.some(segment => segment.id === work.defaultSegmentId))
}
if (!validateError(errorFixture))
  throw new Error(`business error fixture rejected: ${ajv.errorsText(validateError.errors)}`)
const legacyError = structuredClone(errorFixture)
delete legacyError.pluginError.reason
if (!validateError(legacyError)) throw new Error('legacy business error rejected')
for (const property of ['code', 'reason']) {
  for (const value of errorSchema.properties.pluginError.properties[property].enum) {
    const valid = structuredClone(errorFixture)
    valid.pluginError[property] = value
    if (!validateError(valid) || !runtimeContract.includes(`'${value}'`))
      throw new Error(`business error contract mismatch: ${property}`)
  }
}
for (const [name, mutate] of [
  ['unknown reason', value => { value.pluginError.reason = 'provider-supplied-text' }],
  ['URL reason', value => { value.pluginError.reason = 'https://cdn.invalid/?token=secret' }],
  ['object reason', value => { value.pluginError.reason = { token: 'secret' } }],
  ['oversized message', value => { value.pluginError.message = 'x'.repeat(257) }],
  ['upstream body', value => { value.pluginError.body = { token: 'secret' } }],
]) {
  const invalid = structuredClone(errorFixture)
  mutate(invalid)
  if (validateError(invalid)) throw new Error(`invalid business error accepted: ${name}`)
}
if (!validateOffline(offlineFixture))
  throw new Error(`offline fixture rejected: ${ajv.errorsText(validateOffline.errors)}`)
for (const [name, mutate] of [
  ['raw URL', value => { value.assets[0].urlRef = 'https://cdn.example/video.m3u8' }],
  ['credential headers', value => { value.assets[0].headers = { Cookie: 'secret' } }],
  ['path', value => { value.suggestedFileName = '../video.mp4' }],
  ['live format', value => { value.format = 'live' }],
  ['header reference', value => { value.assets[0].headersRef = 'secret' }],
]) {
  const invalid = structuredClone(offlineFixture)
  mutate(invalid)
  if (validateOffline(invalid)) throw new Error(`invalid offline fixture accepted: ${name}`)
}

if (!validate(fixture))
  throw new Error(`valid fixture rejected: ${ajv.errorsText(validate.errors)}`)
if (!validateRegistry(registryFixture))
  throw new Error(`valid registry fixture rejected: ${ajv.errorsText(validateRegistry.errors)}`)
if (!validateRegistry(officialRegistry))
  throw new Error(`official registry rejected: ${ajv.errorsText(validateRegistry.errors)}`)

const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
const fixtureWork = onlineMediaFixture.feed?.[0]?.items?.[0]?.work
if (!validWorkSelection(fixtureWork)) throw new Error('default media selection fixture rejected')
const legacyWork = structuredClone(fixtureWork)
delete legacyWork.defaultSegmentId
if (!validWorkSelection(legacyWork)) throw new Error('legacy media work rejected')
for (const [name, mutate] of [
  ['foreign segment', value => { value.defaultSegmentId = 'another-work-segment' }],
  ['empty segment', value => { value.defaultSegmentId = '' }],
  ['object segment', value => { value.defaultSegmentId = { id: 'segment-main' } }],
  ['oversized segment', value => { value.defaultSegmentId = 'x'.repeat(513) }],
  ['missing segments', value => { delete value.segments }],
  ['empty segments', value => { value.segments = [] }],
]) {
  const invalid = structuredClone(fixtureWork)
  mutate(invalid)
  if (validWorkSelection(invalid)) throw new Error(`invalid media selection accepted: ${name}`)
}
const fixtureVersion = fixtureWork?.segments?.[0]?.versions?.[0]
const fixtureAction = onlineMediaFixture.feed?.[0]?.items?.[0]?.actions?.[0]
const allowedActionIDs = new Set(['favorite.add', 'favorite.remove', 'watch-later.add', 'watch-later.remove', 'follow.add', 'follow.remove', 'like.add', 'like.remove', 'history.remove'])
const fixtureActions = onlineMediaFixture.feed?.[0]?.items?.[0]?.actions ?? []
const playback = onlineMediaFixture.playback
const assetRefs = [
  ...(playback?.assets ?? []),
  ...(playback?.subtitles ?? []),
  ...(playback?.danmaku ?? []),
].map(asset => asset.urlRef)
if (onlineMediaFixture.schemaVersion !== 1
  || fixtureWork?.identity?.value !== 'fixture-work'
  || fixtureWork?.defaultSegmentId !== 'segment-main'
  || fixtureVersion?.delivery !== 'online'
  || fixtureVersion?.variants?.[0]?.id !== 'qn:120'
  || fixtureAction?.id !== 'favorite.add'
  || fixtureActions.some(action => !allowedActionIDs.has(action.id))
  || playback?.selectionToken !== 'selection-fixture-1'
  || playback?.assets?.map(asset => asset.kind).join(',') !== 'dash-video,dash-audio'
  || assetRefs.length !== 4
  || assetRefs.some(reference => !uuidPattern.test(reference))) {
  throw new Error('online media cross-language fixture is invalid')
}

for (const literal of [
  "'resource.search': 14",
  "'resource.resolve': 15",
  "'resource.health': 16",
  "'resource.auth.login': 17",
  "'resource.auth.captcha': 18",
  "'resource.auth.cookie': 19",
  "'media.offline_download_plan': 20",
  "'browser-verification-required'",
  "'captcha-required'",
  "'captcha-expired'",
  "'auth-failed'",
]) {
  if (!runtimeContract.includes(literal))
    throw new Error(`resource-site runtime contract is missing ${literal}`)
}

for (const [name, mutate] of [
  ['unknown capability', value => value.capabilities.push('pt.site')],
  ['path traversal', value => { value.entry = '../plugin.wasm' }],
  ['artwork path traversal', value => { value.libraryArtwork = '../cover.png' }],
  ['active artwork', value => { value.libraryArtwork = 'assets/cover.svg' }],
  ['unknown field', value => { value.serverInternals = true }],
  ['invalid semver', value => { value.version = '1.0.0-beta..1' }],
  ['duplicate permission', value => { value.permissions = [{ kind: 'download.plan' }, { kind: 'download.plan' }] }],
  ['unbounded storage', value => { value.permissions = [{ kind: 'storage.private', maxBytes: 999_999_999 }] }],
]) {
  const invalid = structuredClone(fixture)
  mutate(invalid)
  if (validate(invalid))
    throw new Error(`invalid fixture accepted: ${name}`)
}

for (const [name, mutate] of [
  ['invalid registry semver', value => { value.plugins[0].version = '01.0.0' }],
  ['invalid release path', value => { value.plugins[0].packageUrl = 'https://github.com/ohmycine/example-plugins/releases/download/../plugin.omcp' }],
]) {
  const invalid = structuredClone(registryFixture)
  mutate(invalid)
  if (validateRegistry(invalid))
    throw new Error(`invalid registry accepted: ${name}`)
}

console.log(`plugin contract verified from ${root}`)
