import { Show } from 'solid-js'
import { t } from './i18n'
import type { IndexProgress } from './ipc'

/**
 * The blocking index screen.
 *
 * Covers the whole window and swallows input while a pass runs, because the
 * library is not usable until the pass is done: rows appear as they are indexed,
 * so a list you can see is a list that is missing siblings you cannot see are
 * missing. The user asked for this over a background pass for exactly that reason.
 *
 * The bar is honest about two things a naive one is not:
 *
 * - **The total is unknown until the walk finishes.** The indexer counts files
 *   while walking, so the first ticks arrive with `total: 0`. The bar stays in an
 *   indeterminate state until a total exists rather than jumping from 100% to 3%.
 * - **A finished pass is not an instant.** The last phase (FTS rebuild, prune) has
 *   no per-file progress, so the label switches to "finishing" instead of leaving
 *   the bar pinned at 100% while the window is still blocked.
 *
 * The bar itself is `ix-progress-indicator`; the overlay around it is ours because
 * iX has no blocking full-window variant.
 */
export function IndexOverlay(props: {
  open: boolean
  label: string
  progress: IndexProgress | null
}) {
  const pct = () => {
    const p = props.progress
    if (!p || p.total === 0) return null
    return Math.min(100, Math.round((p.done / p.total) * 100))
  }

  return (
    <Show when={props.open}>
      <div class="index-overlay" role="dialog" aria-busy="true">
        <div class="index-card">
          <div class="index-title">{props.label}</div>

          <ix-progress-indicator
            type="linear"
            value={pct() ?? 0}
            label={pct() === null ? t('index.walking') : `${pct()}%`}
          />

          <div class="index-detail">
            <Show
              when={props.progress && props.progress.total > 0 ? props.progress : null}
              fallback={<span>{t('index.walkingEllipsis')}</span>}
            >
              {(p) => (
                <span>
                  {t('index.files', {
                    done: p().done.toLocaleString(),
                    total: p().total.toLocaleString()
                  })}
                  <Show when={pct() === 100}> · {t('index.finishing')}</Show>
                </span>
              )}
            </Show>
          </div>
        </div>
      </div>
    </Show>
  )
}
