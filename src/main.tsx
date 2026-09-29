import React, { lazy, Suspense } from "react";
import ReactDOM from "react-dom/client";
import Widget from "./components/Widget.js";
import "./styles.css";

// The Tauri window loads index.html#widget; #demo is a browser-only gallery of
// every state. Prefix match so a hash that carries a query still resolves.
const hash = window.location.hash;
const isDemo = hash.startsWith("#demo");
const isSettings = hash.startsWith("#settings");
if (hash.startsWith("#widget")) document.documentElement.classList.add("widget-mode");
if (isDemo) document.documentElement.classList.add("demo-mode");
if (isSettings) document.documentElement.classList.add("settings-mode");

const Demo = lazy(() => import("./components/Demo.js"));
const Settings = lazy(() => import("./settings/Settings.js"));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {isSettings ? (
      <Suspense fallback={null}>
        <Settings />
      </Suspense>
    ) : isDemo ? (
      <Suspense fallback={null}>
        <Demo />
      </Suspense>
    ) : (
      <Widget />
    )}
  </React.StrictMode>,
);
