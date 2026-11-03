// Vite ?raw imports (get a file's content as a string). Used by unit tests to inspect source and CSS.
declare module "*?raw" {
  const content: string;
  export default content;
}

// Vite import.meta.glob, used by unit tests to read every source file as a string
interface ImportMeta {
  glob(pattern: string, options: { query: "?raw"; import: "default"; eager: true }): Record<string, string>;
}
