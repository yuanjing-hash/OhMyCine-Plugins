import { readFile, readdir } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const repositoryRoot = resolve(fileURLToPath(new URL('../..', import.meta.url)))
export const officialRepository = 'yuanjing-hash/OhMyCine-Plugins'
export const officialHomepage = `https://github.com/${officialRepository}`
export const strictSemver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?$/

export function option(name, fallback) {
  const index = process.argv.indexOf(`--${name}`)
  return index >= 0 ? process.argv[index + 1] : fallback
}

export function hasFlag(name) {
  return process.argv.includes(`--${name}`)
}

export async function readJSON(path) {
  return JSON.parse(await readFile(resolve(path), 'utf8'))
}

export function parseCargoPackage(text) {
  const marker = text.indexOf('[package]')
  if (marker < 0) throw new Error('Cargo.toml has no [package] table')
  const remainder = text.slice(marker + '[package]'.length)
  const nextTable = remainder.search(/\r?\n\[/)
  const packageBlock = nextTable >= 0 ? remainder.slice(0, nextTable) : remainder
  const name = packageBlock?.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1]
  const version = packageBlock?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1]
  if (!name || !version || !strictSemver.test(version))
    throw new Error('Cargo.toml has no valid package name/version')
  return { name, version, wasmStem: name.replaceAll('-', '_') }
}

export async function discoverOfficialPlugins() {
  const root = resolve(repositoryRoot, 'plugins/official')
  const entries = await readdir(root, { withFileTypes: true })
  return entries.filter(entry => entry.isDirectory() && /^[a-z0-9-]+$/.test(entry.name)).map(entry => entry.name).sort()
}

export function compareSemver(left, right) {
  const a = strictSemver.exec(left)
  const b = strictSemver.exec(right)
  if (!a || !b) throw new Error(`invalid semver comparison: ${left}, ${right}`)
  for (let index = 1; index <= 3; index += 1) {
    const difference = Number(a[index]) - Number(b[index])
    if (difference !== 0) return Math.sign(difference)
  }
  if (a[4] == null && b[4] == null) return 0
  if (a[4] == null) return 1
  if (b[4] == null) return -1
  const leftParts = a[4].split('.')
  const rightParts = b[4].split('.')
  const count = Math.max(leftParts.length, rightParts.length)
  for (let index = 0; index < count; index += 1) {
    if (leftParts[index] == null) return -1
    if (rightParts[index] == null) return 1
    if (leftParts[index] === rightParts[index]) continue
    const leftNumeric = /^\d+$/.test(leftParts[index])
    const rightNumeric = /^\d+$/.test(rightParts[index])
    if (leftNumeric && rightNumeric) return Number(leftParts[index]) < Number(rightParts[index]) ? -1 : 1
    if (leftNumeric !== rightNumeric) return leftNumeric ? -1 : 1
    return leftParts[index] < rightParts[index] ? -1 : 1
  }
  return 0
}

export function releaseURLs(pluginName, pluginID, version) {
  const tag = `plugin-${pluginName}-v${version}`
  const stem = `${pluginID}-${version}`
  const base = `${officialHomepage}/releases/download/${tag}`
  return {
    tag,
    manifestName: `${stem}.manifest.json`,
    packageName: `${stem}.omcp`,
    checksumName: `${stem}.omcp.sha256`,
    manifestUrl: `${base}/${stem}.manifest.json`,
    packageUrl: `${base}/${stem}.omcp`,
  }
}

export function releaseEntry({ pluginName, manifest, release, digest }) {
  const urls = releaseURLs(pluginName, manifest.id, manifest.version)
  const entry = {
    id: manifest.id,
    name: manifest.name,
    description: manifest.description,
    version: manifest.version,
    channel: release.channel,
    categories: release.categories,
    manifestUrl: urls.manifestUrl,
    packageUrl: urls.packageUrl,
    packageSha256: digest,
    minServerVersion: manifest.minServerVersion,
  }
  if (manifest.maxServerVersion != null) entry.maxServerVersion = manifest.maxServerVersion
  if (release.releaseNotes) entry.releaseNotes = release.releaseNotes
  return { ...urls, entry }
}
