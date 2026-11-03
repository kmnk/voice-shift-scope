// Vite ?raw imports (get a file's content as a string). Used by unit tests to inspect source and CSS.
declare module "*?raw" {
  const content: string;
  export default content;
}
