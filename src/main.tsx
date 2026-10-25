import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/rubik";
import "./styles/app.css";
import App from "./App";
import { platform } from "./lib/util";

// macOS draws the window controls over the content (overlay title bar); Windows and Linux keep a native one.
document.documentElement.dataset.platform = platform;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
