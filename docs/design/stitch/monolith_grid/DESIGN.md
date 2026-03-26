# Design System Document: Technical Sophistication

## 1. Overview & Creative North Star: "The Architectural Monolith"

This design system is built upon the principle of **Architectural Monolithism**. It rejects the soft, bubbly, and rounded trends of the modern web in favor of a brutalist, high-precision aesthetic inspired by high-end editorial print and technical schematics. 

The system moves beyond "standard" minimalism by using **Stark Contrast** and **Intentional Voids**. Instead of using shadows or depth to create hierarchy, we use mathematical precision, razor-sharp edges, and a binary color language. It is designed to feel like a high-performance instrument: efficient, uncompromising, and sophisticated.

**Creative North Star Principles:**
*   **Zero-Degree Radius:** Every corner is a perfect 90-degree angle. This communicates structural integrity and technical precision.
*   **Binary Contrast:** The interplay between `#FFFFFF` and `#000000` is the primary driver of the UI.
*   **Generous Voids:** Whitespace is not "empty"; it is a functional element used to group information without the need for traditional containers.
*   **Editorial Authority:** Typography is treated as the primary graphical element, scaled aggressively to create a sense of hierarchy.

---

## 2. Colors

The palette is a disciplined study in grayscale, utilizing the contrast between pure light and total void.

### Core Palette
*   **Primary (`#000000`):** Used for all primary actions, heavy headers, and structural strokes.
*   **Surface / Background (`#FFFFFF`):** The infinite canvas. Every experience begins here.
*   **Secondary / Neutrals:** We utilize `secondary (#5e5e5e)` and `outline (#777777)` only for low-priority metadata or disabled states to ensure they do not dilute the primary contrast.

### The "No-Shadow" Rule
This system strictly prohibits the use of drop shadows or blurs. Depth is an illusion we do not require. Hierarchy is achieved through:
1.  **Stroke Weight:** A 1px black border (`primary`) defines a boundary.
2.  **Inversion:** High-priority sections should switch to a Black background with White text (`on-primary`) to create immediate visual "gravity."

### Surface Hierarchy
While the system is flat, we use subtle tonal shifts for complex interfaces:
*   **Surface Lowest (`#ffffff`):** Standard page background.
*   **Surface Container (`#eeeeee`):** Used for subtle grouping of secondary technical data.
*   **Surface Dim (`#dadada`):** Used exclusively for inactive or "read-only" background states.

---

## 3. Typography: Inter as Architecture

We use **Inter** for its neutral, technical DNA. The typography must feel "set" like a physical printing press.

| Level | Size | Weight | Tracking | Case |
| :--- | :--- | :--- | :--- | :--- |
| **Display-LG** | 3.5rem | 800 (Bold) | -0.02em | Sentence |
| **Headline-LG** | 2.0rem | 700 (Bold) | -0.01em | Sentence |
| **Title-MD** | 1.125rem | 600 (Semi-Bold) | 0 | Sentence |
| **Body-LG** | 1.0rem | 400 (Regular) | 0 | Sentence |
| **Label-MD** | 0.75rem | 700 (Bold) | +0.05em | ALL CAPS |

**Editorial Direction:** Use `Label-MD` in All Caps for section headers to provide a "technical blueprint" feel. Use `Display-LG` for key data points to make them feel indisputable and authoritative.

---

## 4. Elevation & Precision

In this design system, "Elevation" is replaced by **The Rule of Lines.**

*   **Tonal Layering:** To separate a sidebar from a main content area, do not use a shadow. Use a 1px vertical line (`outline_variant`) or a slight background shift to `surface_container_low`.
*   **The Ghost Border:** For interactive elements that are not currently active, use a 1px border of `outline_variant` (#c6c6c6). Upon hover or focus, this must snap instantly to 1px `primary` (#000000).
*   **Sharp Intersection:** Elements should meet at hard 90-degree angles. When stacking cards or modules, ensure they share a border or are separated by exactly `spacing.4` (1.4rem) to maintain the "grid" feel.

---

## 5. Components

### Buttons
*   **Primary:** Background `#000000`, Text `#FFFFFF`, 0px radius. Padding: `1.4rem` (horizontal) / `0.85rem` (vertical).
*   **Secondary:** Background `Transparent`, 1px Border `#000000`, Text `#000000`, 0px radius.
*   **Tertiary:** Text `#000000`, Underline 1px (on hover only).

### Input Fields
*   **State:** 1px border `#000000` constant. No rounded corners.
*   **Label:** Use `Label-MD` (All Caps) positioned strictly above the field.
*   **Focus:** Increase border weight to 2px or add a solid black "focus block" adjacent to the input.

### Cards & Lists
*   **The No-Divider Rule:** In lists, do not use horizontal lines between every item. Instead, use `spacing.4` to create "air" between items. If a separator is required, use a 4px wide vertical "accent bar" of `#000000` on the far left of the active item.
*   **Nesting:** Place a `surface_container_lowest` (#FFFFFF) card inside a `surface_container` (#EEEEEE) section to create a "tabbed" or "inset" look without shadows.

### Technical Data Modules (Unique Component)
For displaying metrics or logs, use a "Grid Cell" approach: 1px black borders on all sides of a container, with content perfectly centered. This mimics a spreadsheet or a technical terminal.

---

## 6. Do’s and Don'ts

### Do:
*   **Do** embrace the void. If a screen feels "too white," add more padding rather than more lines.
*   **Do** use 90-degree angles for everything, including icons and image containers.
*   **Do** use extreme typographic scale. A very small label next to a very large number creates a "premium" feel.
*   **Do** use instant transitions. Avoid "bouncy" or "soft" easing; use `linear` or `expo-out` for micro-interactions to maintain a technical feel.

### Don't:
*   **Don't** use border-radius. Even 2px is a violation of the system's DNA.
*   **Don't** use "Uber Blue" or any secondary brand colors unless it is a critical system error (`#ba1a1a`).
*   **Don't** use gradients. Every color must be flat and honest.
*   **Don't** use soft grey text for body copy. Use `#1a1c1c` to maintain high legibility and contrast.

---

## 7. Spacing Scale

Strict adherence to the spacing scale is mandatory to maintain the "Architectural" feel.

*   **Micro (0.35rem - 0.7rem):** Internal component spacing (label to input).
*   **Standard (1.4rem):** Default gutter between components.
*   **Macro (4.0rem - 7.0rem):** Section margins. This "over-spacing" is what makes the system feel high-end and sophisticated rather than cluttered.