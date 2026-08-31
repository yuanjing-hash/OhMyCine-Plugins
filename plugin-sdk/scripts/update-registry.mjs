import { writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { compareSemver, officialHomepage, option, readJSON, repositoryRoot } from './repository-utils.mjs'

const registryPath = resolve(option('registry', resolve(repositoryRoot, 'ohmycine-plugin-registry.v1.json')))
const metadata = await readJSON(option('metadata'))
const registry = await readJSON(registryPath)
if (registry.repository.homepage !== officialHomepage)
  throw new Error(`registry homepage must be ${officialHomepage}`)

const incoming = metadata.registryEntry
const index = registry.plugins.findIndex(entry => entry.id === incoming.id)
let changed = false
if (index < 0) {
  registry.plugins.push(incoming)
  changed = true
} else {
  const current = registry.plugins[index]
  const ordering = compareSemver(incoming.version, current.version)
  if (ordering < 0) throw new Error(`refusing Registry rollback for ${incoming.id}: ${current.version} -> ${incoming.version}`)
  if (ordering === 0) {
    if (JSON.stringify(current) !== JSON.stringify(incoming))
      throw new Error(`same-version Registry mutation differs for ${incoming.id} ${incoming.version}`)
  } else {
    registry.plugins[index] = incoming
    changed = true
  }
}

if (changed) {
  registry.plugins.sort((left, right) => left.id.localeCompare(right.id))
  registry.repository.updatedAt = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  await writeFile(registryPath, `${JSON.stringify(registry, null, 2)}\n`)
}
console.log(JSON.stringify({ changed, pluginID: incoming.id, version: incoming.version }))
