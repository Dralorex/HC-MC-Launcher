(() => {
  const NOTE_SURFACES = ["website", "launcher"];

  const UPDATES = [
    {
      version: "0.0.24",
      title: "Sign in once and link your names",
      added: [
        {
          text: "Sign in once and stay signed in on the home page and the launcher page.",
          surfaces: ["website", "launcher"],
        },
        {
          text: "A Home button on your phone opens the Hollow Crown home page.",
          surfaces: ["website"],
        },
        {
          text: "Link your Minecraft name, your Discord name, and your Discord nickname.",
          surfaces: ["website"],
        },
        {
          text: "Each new signup shows a Shift+Enter note you can close.",
          surfaces: ["website"],
        },
        {
          text: "Check the box in that note to keep it off later signups.",
          surfaces: ["website"],
        },
      ],
      updated: [
        {
          text: "Press Enter in a short answer to move to the next question.",
          surfaces: ["website"],
        },
        {
          text: "Press Enter in a long answer to move to the next question.",
          surfaces: ["website"],
        },
        {
          text: "Press Shift+Enter to start a new line in a short or long answer.",
          surfaces: ["website"],
        },
        {
          text: "A short answer can be up to 200 words.",
          surfaces: ["website"],
        },
        {
          text: "A long answer can be up to 1,500 words.",
          surfaces: ["website"],
        },
        {
          text: "While editing a signup, Enter starts a new line in any answer.",
          surfaces: ["website"],
        },
        {
          text: "Clicking a choice selects it even while you are typing.",
          surfaces: ["website"],
        },
      ],
      removed: [],
    },
  ];

  const SEEN_PREFIX = "hc-update-notes:";

  function notesForSurface(updates, surface) {
    return updates.flatMap((update) => {
      const added = update.added.filter((note) => note.surfaces.includes(surface));
      const updated = update.updated.filter((note) => note.surfaces.includes(surface));
      const removed = update.removed.filter((note) => note.surfaces.includes(surface));
      if (added.length + updated.length + removed.length === 0) return [];
      return [{ ...update, added, updated, removed }];
    });
  }

  function seenKey(surface, version) {
    return `${SEEN_PREFIX}${surface}:${version}`;
  }

  function readSeen(surface, version) {
    try {
      return window.localStorage.getItem(seenKey(surface, version)) === "1";
    } catch {
      return false;
    }
  }

  function writeSeen(surface, version) {
    try {
      window.localStorage.setItem(seenKey(surface, version), "1");
    } catch {
      /* Closing still works when storage is blocked. */
    }
  }

  function noteList(heading, items) {
    if (items.length === 0) return "";
    return `
      <section>
        <h3>${heading}</h3>
        <ul>
          ${items.map((item) => `<li>${item.text}</li>`).join("")}
        </ul>
      </section>
    `;
  }

  function ensureStyles() {
    if (document.getElementById("hc-update-notes-style")) return;
    const style = document.createElement("style");
    style.id = "hc-update-notes-style";
    style.textContent = `
      .update-notes {
        margin-top: 2rem;
      }
      .update-notes > button {
        appearance: none;
        cursor: pointer;
        padding: 0.55rem 0.9rem;
        border-radius: 0.5rem;
        border: 1px solid var(--line);
        background: var(--raised);
        color: var(--text);
        font: inherit;
        font-weight: 600;
      }
      .update-notes-backdrop {
        position: fixed;
        inset: 0;
        z-index: 30;
        display: grid;
        place-items: center;
        padding: 1rem;
        background: rgba(0, 0, 0, 0.62);
      }
      .update-notes-panel {
        width: min(36rem, 100%);
        max-height: min(85vh, 40rem);
        overflow: auto;
        background: var(--raised);
        border: 1px solid var(--line);
        border-radius: 0.75rem;
        padding: 1.1rem 1.2rem 1.2rem;
        color: var(--text);
      }
      .update-notes-panel .kicker {
        margin: 0;
        color: var(--muted);
        font-size: 0.95rem;
        font-weight: 600;
      }
      .update-notes-panel article + article {
        margin-top: 1.25rem;
        padding-top: 1.25rem;
        border-top: 1px solid var(--line);
      }
      .update-notes-panel h2 {
        margin: 0.35rem 0 0.25rem;
        font-size: 1.35rem;
        line-height: 1.2;
      }
      .update-notes-panel article > p {
        margin: 0 0 0.85rem;
        color: var(--muted);
      }
      .update-notes-panel h3 {
        margin: 0.85rem 0 0.35rem;
        font-size: 0.95rem;
      }
      .update-notes-panel ul {
        margin: 0;
        padding-left: 1.15rem;
      }
      .update-notes-panel li + li {
        margin-top: 0.35rem;
      }
      .update-notes-panel > button {
        appearance: none;
        cursor: pointer;
        margin-top: 1rem;
        padding: 0.55rem 0.9rem;
        border-radius: 0.5rem;
        border: 1px solid var(--line);
        background: #2a2622;
        color: var(--text);
        font: inherit;
        font-weight: 600;
      }
    `;
    document.head.appendChild(style);
  }

  function mount(surface) {
    const visible = notesForSurface(UPDATES, surface);
    const latest = visible[0];
    if (!latest) return;

    ensureStyles();

    const root = document.createElement("div");
    root.className = "update-notes";

    const openButton = document.createElement("button");
    openButton.type = "button";
    openButton.textContent = "What's new";
    root.appendChild(openButton);

    let backdrop = null;
    let closeButton = null;
    let keyHandler = null;

    function closePanel() {
      writeSeen(surface, latest.version);
      if (keyHandler) {
        window.removeEventListener("keydown", keyHandler);
        keyHandler = null;
      }
      if (backdrop) {
        backdrop.remove();
        backdrop = null;
        closeButton = null;
      }
      openButton.focus();
    }

    function openPanel() {
      if (backdrop) return;

      backdrop = document.createElement("div");
      backdrop.className = "update-notes-backdrop";
      backdrop.addEventListener("mousedown", (event) => {
        if (event.target === backdrop) closePanel();
      });

      const panel = document.createElement("div");
      panel.className = "update-notes-panel";
      panel.setAttribute("role", "dialog");
      panel.setAttribute("aria-modal", "true");
      panel.setAttribute("aria-labelledby", "hc-update-notes-title");

      panel.innerHTML = `
        <p class="kicker">What's new</p>
        ${visible
          .map(
            (update, index) => `
          <article>
            <h2 ${index === 0 ? 'id="hc-update-notes-title"' : ""}>UP #${update.version}</h2>
            <p>${update.title}</p>
            ${noteList("Added", update.added)}
            ${noteList("Updated", update.updated)}
            ${noteList("Removed", update.removed)}
          </article>
        `,
          )
          .join("")}
      `;

      closeButton = document.createElement("button");
      closeButton.type = "button";
      closeButton.textContent = "Close";
      closeButton.addEventListener("click", closePanel);
      panel.appendChild(closeButton);

      backdrop.appendChild(panel);
      document.body.appendChild(backdrop);
      closeButton.focus();

      keyHandler = (event) => {
        if (event.key === "Escape") closePanel();
      };
      window.addEventListener("keydown", keyHandler);
    }

    openButton.addEventListener("click", openPanel);

    const host = document.querySelector("main") || document.body;
    const footer = host.querySelector("footer");
    if (footer) {
      host.insertBefore(root, footer);
    } else {
      host.appendChild(root);
    }

    if (!readSeen(surface, latest.version)) {
      openPanel();
    }
  }

  const script = document.currentScript;
  const surface = script?.getAttribute("data-surface");
  if (!NOTE_SURFACES.includes(surface)) return;

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => mount(surface));
  } else {
    mount(surface);
  }
})();
