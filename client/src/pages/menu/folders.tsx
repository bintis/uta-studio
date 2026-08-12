import { listLibraryFolder, openLibraryEntry, revealLibraryEntry } from "@/bridge/source";
import { Button } from "@/components/ui/button";
import { useLibrarySourceActions } from "@/hooks/use-library-source-actions";
import type { LibraryFolderEntry } from "@/types/LibraryFolderEntry";
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from "@/components/ui/context-menu";
import { formatBytes } from "@/utils/stats";
import { useQuery } from "@tanstack/react-query";
import {
  ChevronLeft,
  FileMusic,
  FileText,
  Film,
  Folder,
  FolderOpen,
  ListMusic,
  LoaderCircle,
  Plus,
  RefreshCw,
  ExternalLink,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

const basename = (path: string) => {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
};
const parentOf = (path: string) => {
  const clean = path.replace(/[\\/]+$/, "");
  const cut = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  return cut > 0 ? clean.slice(0, cut) : clean;
};

const EntryIcon = ({ entry }: { entry: LibraryFolderEntry }) => {
  if (entry.kind === "folder") return <Folder className="size-4 text-amber-500/80" />;
  if (entry.kind === "video") return <Film className="size-4 text-cyan-500/75" />;
  if (entry.kind === "playlist") return <ListMusic className="size-4 text-emerald-500/75" />;
  if (entry.kind === "chart") return <FileText className="size-4 text-violet-500/75" />;
  return <FileMusic className="size-4 text-primary/75" />;
};

export const FoldersPage = () => {
  const source = useLibrarySourceActions();
  const [root, setRoot] = useState<string | null>(null);
  const [current, setCurrent] = useState<string | null>(null);

  useEffect(() => {
    if (root && source.paths.includes(root)) return;
    const next = source.paths[0] ?? null;
    setRoot(next);
    setCurrent(next);
  }, [root, source.paths]);

  const folderQuery = useQuery({
    queryKey: ["library-folder", current],
    queryFn: () => listLibraryFolder(current!),
    enabled: Boolean(current),
  });
  const parent = useMemo(() => (current ? parentOf(current) : null), [current]);
  const canGoUp = Boolean(
    root && current && parent && current !== root && parent.length >= root.length,
  );

  const chooseRoot = (path: string) => {
    setRoot(path);
    setCurrent(path);
  };

  const runFileAction = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (error) {
      toast.error("Could not open this library item", { description: String(error) });
    }
  };

  return (
    <main className="roon-library min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-7 lg:px-10 lg:py-8">
      <header className="mx-auto flex max-w-7xl flex-wrap items-end justify-between gap-4 border-b border-border/45 pb-5">
        <div>
          <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
            My library
          </p>
          <h1 className="mt-1 text-2xl font-light">Folders</h1>
          <p className="mt-1 text-[10px] text-muted-foreground">
            Browse media in every watched location. Uta Studio never moves or deletes your files.
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" disabled={source.rescanDisabled} onClick={source.rescan}>
            <RefreshCw /> Rescan all
          </Button>
          <Button disabled={source.isPending} onClick={source.selectFolder}>
            <Plus /> Add folder
          </Button>
        </div>
      </header>

      <div className="mx-auto mt-5 grid max-w-7xl gap-4 lg:grid-cols-[15rem_minmax(0,1fr)]">
        <aside className="glass-panel h-fit rounded-md p-2">
          <p className="px-2 pb-2 pt-1 text-[8px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
            Watched locations · {source.paths.length}
          </p>
          {source.paths.length === 0 ? (
            <div className="px-2 py-7 text-center text-[10px] text-muted-foreground">
              No folders added yet.
            </div>
          ) : (
            <div className="space-y-0.5">
              {source.paths.map((path) => (
                <div
                  key={path}
                  className={`group flex items-center rounded-sm transition ${root === path ? "bg-foreground/[0.07] text-foreground" : "text-muted-foreground hover:bg-foreground/[0.035] hover:text-foreground"}`}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 px-2 py-2 text-left"
                    onClick={() => chooseRoot(path)}
                  >
                    <FolderOpen className="size-4 shrink-0" />
                    <span className="min-w-0">
                      <span className="block truncate text-xs">{basename(path)}</span>
                      <span className="block truncate text-[9px] opacity-56">{path}</span>
                    </span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="mr-1 opacity-0 group-hover:opacity-65 focus-visible:opacity-100"
                    aria-label={`Remove ${path}`}
                    disabled={source.isPending}
                    onClick={() => {
                      if (
                        window.confirm("Stop watching this folder? Your media will not be deleted.")
                      )
                        source.removeFolder(path);
                    }}
                  >
                    <Trash2 />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </aside>

        <section className="glass-panel min-h-[24rem] overflow-hidden rounded-md">
          <div className="flex min-h-12 items-center gap-2 border-b border-border/45 px-3">
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={!canGoUp}
              aria-label="Parent folder"
              onClick={() => parent && setCurrent(parent)}
            >
              <ChevronLeft />
            </Button>
            <Folder className="size-4 text-muted-foreground" />
            <span className="min-w-0 truncate text-[11px] text-muted-foreground">
              {current ?? "Choose a watched folder"}
            </span>
            {folderQuery.isFetching ? (
              <LoaderCircle className="ml-auto size-3.5 animate-spin text-muted-foreground" />
            ) : null}
          </div>

          {!current ? (
            <div className="grid min-h-80 place-items-center text-center">
              <div>
                <FolderOpen className="mx-auto size-8 text-primary/65" />
                <p className="mt-3 text-sm font-medium">Add a music folder to begin</p>
                <Button className="mt-4" onClick={source.selectFolder}>
                  <Plus /> Add folder
                </Button>
              </div>
            </div>
          ) : folderQuery.isError ? (
            <div className="grid min-h-80 place-items-center px-6 text-center text-xs text-destructive">
              Could not read this folder: {String(folderQuery.error)}
            </div>
          ) : (
            <div role="table" aria-label="Folder contents" className="text-[11px]">
              <div
                role="row"
                className="grid grid-cols-[minmax(0,1fr)_6rem_7rem] border-b border-border/35 px-4 py-2 text-[9px] text-muted-foreground"
              >
                <span>Name</span>
                <span>Kind</span>
                <span className="text-right">Size</span>
              </div>
              {folderQuery.data?.map((entry) => (
                <ContextMenu key={entry.path}>
                  <ContextMenuTrigger asChild>
                    <button
                      type="button"
                      role="row"
                      className="grid w-full grid-cols-[minmax(0,1fr)_6rem_7rem] items-center border-b border-border/28 px-4 py-2 text-left transition hover:bg-foreground/[0.035]"
                      onDoubleClick={() =>
                        entry.kind === "folder"
                          ? setCurrent(entry.path)
                          : void runFileAction(() => openLibraryEntry(entry.path))
                      }
                      onClick={() => entry.kind === "folder" && setCurrent(entry.path)}
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <EntryIcon entry={entry} />
                        <span className="truncate">{entry.name}</span>
                      </span>
                      <span className="capitalize text-muted-foreground">{entry.kind}</span>
                      <span className="text-right tabular-nums text-muted-foreground">
                        {entry.size_bytes ? formatBytes(entry.size_bytes) : "—"}
                      </span>
                    </button>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="song-actions-popover w-56 p-1.5">
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 rounded-sm px-2.5 py-2 text-left text-xs text-foreground hover:bg-foreground/[0.055]"
                      onClick={() =>
                        entry.kind === "folder"
                          ? setCurrent(entry.path)
                          : void runFileAction(() => openLibraryEntry(entry.path))
                      }
                    >
                      <ExternalLink className="size-3.5 text-muted-foreground" />
                      {entry.kind === "folder" ? "Open folder" : "Open with default app"}
                    </button>
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 rounded-sm px-2.5 py-2 text-left text-xs text-foreground hover:bg-foreground/[0.055]"
                      onClick={() => void runFileAction(() => revealLibraryEntry(entry.path))}
                    >
                      <Search className="size-3.5 text-muted-foreground" /> Show in file manager
                    </button>
                  </ContextMenuContent>
                </ContextMenu>
              ))}
              {folderQuery.data?.length === 0 ? (
                <div className="px-4 py-16 text-center text-[10px] text-muted-foreground">
                  No supported media in this folder.
                </div>
              ) : null}
            </div>
          )}
        </section>
      </div>
    </main>
  );
};
