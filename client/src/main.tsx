import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import utaStudioIconUrl from "../src-tauri/icons/icon.png";

const favicon = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
if (favicon) favicon.href = utaStudioIconUrl;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
