// Custom-element typing for the Siemens iX components this UI uses.
//
// iX v5 ships Stencil web components with their own `.d.ts` for the element
// interfaces, but Solid's JSX namespace does not know about them, so every tag
// would need a `@ts-expect-error`. Declaring the handful actually used here keeps
// the components type-checked and the JSX clean.
//
// The property names follow iX's public API (`variant`, `icon`, `size`, …), not
// the attribute spellings: Solid sets properties on custom elements when the name
// is in this map, which is what iX expects for anything that is not a string.
// Verified in the browser: a plain `value={signal()}` binding updates the
// component when the signal changes, so no `prop:` prefix is needed.
import type { JSX as SolidJSX } from 'solid-js'

/**
 * Props every iX element accepts.
 *
 * `DOMAttributes` supplies Solid's own event handlers (`onClick`, `onPointerDown`,
 * …), `class`, `style` and `children`. iX's `on:` CustomEvent handlers are declared
 * separately: they go through `addEventListener`, so the detail arrives on
 * `event.detail`.
 */
type IxBase = SolidJSX.DOMAttributes<HTMLElement> & {
  id?: string
  slot?: string
  title?: string
  disabled?: boolean
  /** iX inherits ARIA attributes onto its inner control, so these are typed. */
  'aria-label'?: string
  'aria-describedby'?: string
  'aria-labelledby'?: string
  role?: string
}

type IxEvents = {
  [K in `on:${string}`]?: (event: CustomEvent<never>) => void
}

type IxElement<P = Record<string, unknown>> = IxBase & IxEvents & P

declare module 'solid-js' {
  namespace JSX {
    interface IntrinsicElements {
      'ix-button': IxElement<{
        variant?:
          | 'primary'
          | 'secondary'
          | 'tertiary'
          | 'danger-primary'
          | 'danger-secondary'
          | 'danger-tertiary'
        type?: 'button' | 'submit' | 'reset'
        icon?: string
        iconRight?: string
        loading?: boolean
        ghost?: boolean
        outline?: boolean
      }>
      'ix-icon-button': IxElement<{
        variant?:
          | 'primary'
          | 'secondary'
          | 'tertiary'
          | 'danger-primary'
          | 'danger-secondary'
          | 'danger-tertiary'
        icon?: string
        size?: '12' | '16' | '24'
        ghost?: boolean
        outline?: boolean
        oval?: boolean
        selected?: boolean
        loading?: boolean
      }>
      'ix-icon': IxElement<{ name?: string; size?: '12' | '16' | '24' | '32' | '48' }>
      'ix-slider': IxElement<{
        min?: number
        max?: number
        step?: number
        value?: number
        trace?: boolean
        traceReference?: number
        disabled?: boolean
        label?: string
        marker?: unknown
      }>
      'ix-input': IxElement<{
        value?: string
        placeholder?: string
        type?: string
        disabled?: boolean
        readonly?: boolean
        helperText?: string
        label?: string
      }>
      'ix-toggle': IxElement<{ checked?: boolean; disabled?: boolean; label?: string }>
      'ix-toggle-button': IxElement<{
        variant?: 'primary' | 'secondary' | 'tertiary'
        pressed?: boolean
        disabled?: boolean
        icon?: string
      }>
      'ix-chip': IxElement<{
        variant?: 'primary' | 'alarm' | 'critical' | 'warning' | 'success' | 'info' | 'neutral' | 'custom'
        outline?: boolean
        closable?: boolean
        icon?: string
        active?: boolean
        inactive?: boolean
      }>
      'ix-pill': IxElement<{
        variant?: 'primary' | 'alarm' | 'critical' | 'warning' | 'info' | 'neutral' | 'success' | 'custom'
        outline?: boolean
        icon?: string
        alignLeft?: boolean
      }>
      'ix-dropdown': IxElement<{
        trigger?: string
        placement?: string
        anchor?: string
        closeBehavior?: string
        show?: boolean
        header?: string
      }>
      'ix-dropdown-item': IxElement<{ label?: string; icon?: string; disabled?: boolean }>
      'ix-modal': IxElement<{
        size?: '360' | '480' | '600' | '720' | '840' | 'full-width' | 'full-screen'
      }>
      'ix-modal-header': IxElement<{ heading?: string; subHeading?: string }>
      'ix-modal-content': IxElement
      'ix-modal-footer': IxElement
      'ix-select': IxElement<{
        value?: string
        disabled?: boolean
        allowClear?: boolean
        mode?: 'single' | 'multiple'
        label?: string
      }>
      'ix-select-item': IxElement<{ value?: string; label?: string; disabled?: boolean }>
      'ix-progress-indicator': IxElement<{
        type?: 'linear' | 'circular'
        size?: 'sm' | 'md' | 'lg'
        value?: number
        min?: number
        max?: number
        label?: string
      }>
      'ix-spinner': IxElement<{ size?: 'sm' | 'md' | 'lg' }>
      'ix-divider': IxElement<{ orientation?: 'horizontal' | 'vertical' }>
      'ix-empty-state': IxElement<{ header?: string; subHeader?: string; icon?: string; layout?: string }>
      'ix-message-bar': IxElement<{
        type?: 'info' | 'warning' | 'error' | 'success' | 'alarm' | 'critical'
      }>
      'ix-blind': IxElement<{ header?: string; collapsed?: boolean; subHeader?: string }>
      'ix-radio-group': IxElement<{ value?: string; label?: string }>
      'ix-radio': IxElement<{ value?: string; label?: string; checked?: boolean }>
      'ix-checkbox': IxElement<{ checked?: boolean; indeterminate?: boolean; label?: string }>
      'ix-tooltip': IxElement<{ for?: string; placement?: string }>
    }
  }
}

export {}
