import type { Transition, Variants } from 'motion/react'

/** Heavy, settling curve used for everything. Never linear / ease-in-out. */
export const EASE = [0.32, 0.72, 0, 1] as const

export const spring: Transition = { type: 'spring', stiffness: 420, damping: 34, mass: 0.9 }
export const softSpring: Transition = { type: 'spring', stiffness: 220, damping: 28, mass: 1 }

/** Fade-up used for staggered reveals. Only transform + opacity. */
export const rise: Variants = {
  hidden: { opacity: 0, y: 20 },
  show: (i: number = 0) => ({
    opacity: 1,
    y: 0,
    transition: { duration: 0.7, ease: EASE, delay: Math.min(i, 12) * 0.055 },
  }),
}

export const pageVariants: Variants = {
  initial: { opacity: 0, y: 16 },
  animate: { opacity: 1, y: 0, transition: { duration: 0.6, ease: EASE } },
  exit: { opacity: 0, y: -8, transition: { duration: 0.18, ease: EASE } },
}
