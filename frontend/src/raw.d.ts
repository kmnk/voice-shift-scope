// Vite の ?raw の読み込み（ファイルの中身を文字列で得る）。単体の試験でソースと CSS を検査するのに使う。
declare module "*?raw" {
  const content: string;
  export default content;
}
