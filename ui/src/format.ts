/**
 * Formatting helpers shared by the table, the dock and the inspector.
 *
 * These lived in `SoundTable` while the table was the only place that printed a
 * size or a rate. The inspector prints the same values for one row, and two
 * copies of "kB or MB" drift the moment one of them is fixed.
 */

/** `512 B`, `48 KB`, `1.2 MB`. `—` when the probe found no size. */
export function formatBytes(bytes: number | null): string {
  if (bytes === null) return '—'
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb.toFixed(0)} KB`
  return `${(kb / 1024).toFixed(1)} MB`
}

/** `48k`, `44.1k`: the table's column is 62px, so the unit is folded in. */
export function formatRate(rate: number | null): string {
  if (rate === null) return '—'
  return `${(rate / 1000).toFixed(rate % 1000 === 0 ? 0 : 1)}k`
}

/** `48 kHz`, `44.1 kHz`: the inspector has the room for the unit. */
export function sampleRateLabel(rate: number | null): string {
  if (rate === null) return '—'
  const decimals = rate % 1000 === 0 ? 0 : rate % 100 === 0 ? 1 : 2
  return `${(rate / 1000).toFixed(decimals)} kHz`
}

/**
 * `128`, or `126.5` for the packs that write half tempos, or an em dash when the
 * name states none. More than one decimal is never a real tempo.
 */
export function formatBpm(bpm: number | null): string {
  if (bpm === null) return '—'
  return Number.isInteger(bpm) ? String(bpm) : bpm.toFixed(1)
}
