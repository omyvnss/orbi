// Agentation feedback toolbar (https://github.com/benjitaylor/agentation).
// Development only: click any element on the page, leave a note, and it syncs
// to the coding agent through the local agentation-mcp server on :4747.
// On any real host this file returns immediately and loads nothing.
const LOCAL = ["localhost", "127.0.0.1", "::1", "[::1]"];

if (LOCAL.includes(location.hostname)) {
  const REACT = "18.3.1";
  const [{ default: React }, { createRoot }, { Agentation }] = await Promise.all([
    import(`https://esm.sh/react@${REACT}`),
    import(`https://esm.sh/react-dom@${REACT}/client`),
    import(`https://esm.sh/agentation@3.1.2?deps=react@${REACT},react-dom@${REACT}`),
  ]);
  const host = document.createElement("div");
  host.id = "agentation-root";
  document.body.appendChild(host);
  createRoot(host).render(React.createElement(Agentation, { endpoint: "http://localhost:4747" }));
}
