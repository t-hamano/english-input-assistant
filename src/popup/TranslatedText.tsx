import { invoke } from "@tauri-apps/api/core";
import { Popover } from "@wordpress/ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PlayIcon } from "../icons";

type WordTranslation =
  | { status: "loading" }
  | { status: "done"; text: string }
  | { status: "error"; message: string };

interface Props {
  text: string;
  playingText: string | null;
  onPlay: (text: string) => void;
}

export function TranslatedText({ text, playingText, onPlay }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const requestRef = useRef(0);
  const [selection, setSelection] = useState<{ word: string; range: Range } | null>(null);
  const [translation, setTranslation] = useState<WordTranslation>({ status: "loading" });

  const close = useCallback(() => {
    // Drop responses for a selection that is no longer shown.
    requestRef.current += 1;
    setSelection(null);
  }, []);

  // A refined sentence invalidates the selected range.
  // biome-ignore lint/correctness/useExhaustiveDependencies: close when the text changes
  useEffect(() => close, [text, close]);

  const handleMouseUp = useCallback(() => {
    const current = window.getSelection();
    if (!current || current.isCollapsed || current.rangeCount === 0) return;
    const range = current.getRangeAt(0);
    if (!ref.current?.contains(range.commonAncestorContainer)) return;
    const word = current.toString().trim();
    if (!word) return;
    const id = ++requestRef.current;
    // Open after the click that ends this mouseup, so it is not treated as an outside press.
    setTimeout(() => {
      if (id !== requestRef.current) return;
      setSelection({ word, range: range.cloneRange() });
      setTranslation({ status: "loading" });
    });
    invoke<string>("translate_word", { text: word, context: text }).then(
      (result) => {
        if (id === requestRef.current) setTranslation({ status: "done", text: result });
      },
      (error) => {
        if (id === requestRef.current) setTranslation({ status: "error", message: String(error) });
      },
    );
  }, [text]);

  const anchor = useMemo(
    () => selection && { getBoundingClientRect: () => selection.range.getBoundingClientRect() },
    [selection],
  );

  const playing = selection !== null && playingText === selection.word;

  return (
    <>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: mouse text selection only; the text stays readable without it */}
      <div ref={ref} className="translated-text" onMouseUp={handleMouseUp}>
        {text}
      </div>
      <Popover.Root
        open={selection !== null}
        onOpenChange={(open) => {
          if (!open) close();
        }}
      >
        <Popover.Popup
          initialFocus={false}
          positioner={
            <Popover.Positioner anchor={anchor ?? undefined} side="bottom" sideOffset={8} />
          }
        >
          {selection && (
            <div className="word-popover">
              <Popover.Close className="word-popover-close" aria-label="閉じる">
                &#x2715;
              </Popover.Close>
              <Popover.Title className="word-popover-word">{selection.word}</Popover.Title>
              <Popover.Description
                className={translation.status === "error" ? "error-msg" : undefined}
              >
                {translation.status === "loading"
                  ? "翻訳中..."
                  : translation.status === "done"
                    ? translation.text
                    : translation.message}
              </Popover.Description>
              <button
                type="button"
                className="word-popover-play"
                aria-label={playing ? "停止" : "再生"}
                aria-pressed={playing || undefined}
                onClick={() => onPlay(selection.word)}
              >
                <PlayIcon />
                <span>{playing ? "停止" : "再生"}</span>
              </button>
            </div>
          )}
        </Popover.Popup>
      </Popover.Root>
    </>
  );
}
