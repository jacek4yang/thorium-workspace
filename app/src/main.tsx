import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { initI18n } from "./i18n";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element in index.html");
}

// Localization is ready before the first render so no default-language
// flash occurs on a zh-CN system.
void initI18n().then(() => {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});
