import type { en } from "./en";

/* The dictionary shape. Every language file must satisfy this exact structure,
   so a missing or renamed key is a compile error. */
export type Dict = typeof en;
