import { createApp } from "vue";
import App from "./App.vue";
import Settings from "./Settings.vue";
import "./styles.css";

const view = new URLSearchParams(window.location.search).get("view");

if (view === "settings") {
  document.body.classList.add("settings-window");
  createApp(Settings).mount("#app");
} else {
  createApp(App).mount("#app");
}
