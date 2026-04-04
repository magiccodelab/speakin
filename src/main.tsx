import ReactDOM from "react-dom/client";
import "./index.css";

const root = document.getElementById("root")!;

// The overlay window loads with #overlay hash
if (window.location.hash === "#overlay") {
  // Transparent background for the overlay window
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";

  import("./components/RecordingOverlay").then(({ RecordingOverlay }) => {
    ReactDOM.createRoot(root).render(<RecordingOverlay />);
  });
} else {
  import("./App").then(({ default: App }) => {
    ReactDOM.createRoot(root).render(<App />);
  });
}
