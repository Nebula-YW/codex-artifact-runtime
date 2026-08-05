import { Braces, Boxes, TerminalSquare } from "lucide-react";

import "./style.css";

const boundaries = [
  {
    icon: TerminalSquare,
    title: "Codex native Code Mode",
    detail: "Dynamic Tools are injected at thread creation.",
  },
  {
    icon: Braces,
    title: "Shared capability contract",
    detail: "One JSON Schema projection for Codex and TSX.",
  },
  {
    icon: Boxes,
    title: "Sandboxed TSX Application",
    detail: "The browser receives only a typed tools proxy.",
  },
];

export default function App() {
  return (
    <main>
      <p className="eyebrow">Runtime foundation</p>
      <h1>Codex Artifact Runtime</h1>
      <p className="intro">
        Native Codex orchestration and interactive TSX Applications now share the same capability
        identity through a single Host-controlled contract.
      </p>
      <section>
        {boundaries.map(({ icon: Icon, title, detail }) => (
          <article key={title}>
            <Icon aria-hidden="true" size={20} />
            <h2>{title}</h2>
            <p>{detail}</p>
          </article>
        ))}
      </section>
    </main>
  );
}
