import {
  SidebarContent,
  SidebarGroup,
  SidebarMenu,
  SidebarMenuSkeleton,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubItem,
  useSidebar,
} from "@/components/ui/sidebar";
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from "@/components/ui/collapsible";
import {
  ChevronDown,
  BadgeCheck,
  Folder,
  FileQuestionMark,
  DiscIcon,
  FileMusic,
  House,
  ListMusicIcon,
  ListTodo,
  UserIcon,
  Video,
  type LucideIcon,
} from "lucide-react";
import { useLibraryMenuItems } from "@/queries/use-library-menu-items";
import type { LibraryMenuItem } from "@/types/LibraryMenuItem";
import { Fragment, useCallback, useEffect, useMemo } from "react";
import { Badge } from "@/components/ui/badge";
import {
  isLibraryMenuItemActive,
  libraryFilterFromMenuSelection,
  type LibraryMenuSection,
} from "@/lib/library-menu-filter";
import type { LibraryMenuFilters } from "@/types/LibraryMenuFilters";
import { useLibraryFilter } from "@/hooks/use-library-filter";
import { usePersistentScroll } from "@/hooks/use-persistent-scroll";
import { useSidebarSectionsOpen } from "@/hooks/use-sidebar-sections-open";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import {
  SidebarNavProvider,
  useSidebarRowFocus,
  useSidebarRouteFocus,
  type SidebarNavRow,
} from "@/contexts/sidebar-nav-context";
import { useNavigate } from "react-router";
import { useIsMobile } from "@/hooks/use-is-mobile";
import { useLocation } from "react-router";

type NavSectionConfig = {
  section: LibraryMenuSection;
  label: string;
  icon: LucideIcon;
};

const NAV_SECTIONS: NavSectionConfig[] = [
  { section: "hot", label: "Browse", icon: House },
  { section: "no_metadata", label: "No Metadata", icon: FileQuestionMark },
  { section: "artists", label: "Artists", icon: UserIcon },
  { section: "albums", label: "Albums", icon: DiscIcon },
  { section: "playlists", label: "Playlists", icon: ListMusicIcon },
];

interface MenuItemCountsProps {
  item: LibraryMenuItem;
}

function MenuItemCounts({ item }: MenuItemCountsProps) {
  if (item.count === 0n) return null;

  return (
    <Badge
      className="h-4 min-w-4 shrink-0 border-0 bg-transparent px-0 text-[0.5rem] font-medium leading-none text-sidebar-foreground/38 shadow-none"
      title={`${item.count} tracks`}
    >
      {item.count.toString()}
    </Badge>
  );
}

interface LibraryNavSubItemProps {
  section: LibraryMenuSection;
  item: LibraryMenuItem;
  filter: LibraryMenuFilters;
  onSelectItem: (section: LibraryMenuSection, item: LibraryMenuItem) => void;
}

function LibraryNavSubItem({ section, item, filter, onSelectItem }: LibraryNavSubItemProps) {
  const { isSidebarActive, isItemFocused, itemIndex } = useSidebarRowFocus(section, item.value);
  return (
    <SidebarMenuSubItem>
      <SidebarMenuButton
        data-sidebar-nav-index={itemIndex}
        isActive={isLibraryMenuItemActive(section, item, filter)}
        className={`flex h-fit items-center justify-between gap-2 rounded-none px-2 py-1.5 hover:bg-transparent hover:text-sidebar-foreground hover:ring-0 ${
          isSidebarActive && isItemFocused ? "bg-transparent text-primary ring-0" : ""
        }`}
        onClick={() => onSelectItem(section, item)}
      >
        {item.label}
        <MenuItemCounts item={item} />
      </SidebarMenuButton>
    </SidebarMenuSubItem>
  );
}

const HOT_ITEM_ICONS: Record<string, LucideIcon> = {
  all: House,
  queued: ListTodo,
  analysed: BadgeCheck,
  videos: Video,
  usdx: FileMusic,
};

interface FlatLibraryNavItemProps extends LibraryNavSubItemProps {
  icon?: LucideIcon;
}

function FlatLibraryNavItem({
  section,
  item,
  filter,
  onSelectItem,
  icon: Icon,
}: FlatLibraryNavItemProps) {
  const { isSidebarActive, isItemFocused, itemIndex } = useSidebarRowFocus(section, item.value);

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        data-sidebar-nav-index={itemIndex}
        isActive={isLibraryMenuItemActive(section, item, filter)}
        className={`flex h-8 items-center justify-between gap-2 rounded-none px-2 text-xs hover:bg-transparent hover:ring-0 ${
          isSidebarActive && isItemFocused ? "bg-transparent text-primary ring-0" : ""
        }`}
        onClick={() => onSelectItem(section, item)}
      >
        <span className="flex min-w-0 items-center gap-2">
          {Icon ? <Icon className="size-4 shrink-0" /> : null}
          <span className="truncate">{item.label}</span>
        </span>
        <MenuItemCounts item={item} />
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

function FoldersNavItem({ onSelect }: { onSelect: () => void }) {
  const { itemIndex, isFocused } = useSidebarRouteFocus("folders");
  const location = useLocation();
  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        data-sidebar-nav-index={itemIndex}
        isActive={location.pathname === "/folders"}
        className={`flex h-8 items-center gap-2 rounded-none px-2 text-xs hover:bg-transparent hover:ring-0 ${isFocused ? "bg-transparent text-primary ring-0" : ""}`}
        onClick={onSelect}
      >
        <Folder className="size-4" /> Folders
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

interface LibraryNavSectionProps extends NavSectionConfig {
  items: LibraryMenuItem[];
  filter: LibraryMenuFilters;
  open: boolean;
  onToggleOpen: (open: boolean) => void;
  onSelectItem: (section: LibraryMenuSection, item: LibraryMenuItem) => void;
}

function LibraryNavSection({
  section,
  label,
  icon: Icon,
  items,
  filter,
  open,
  onToggleOpen,
  onSelectItem,
}: LibraryNavSectionProps) {
  const { isSidebarActive, isCollapseFocused, collapseIndex } = useSidebarRowFocus(section);

  return (
    <Collapsible open={open} onOpenChange={onToggleOpen} className="group/collapsible">
      <SidebarMenuItem>
        <CollapsibleTrigger asChild>
          <SidebarMenuButton
            data-sidebar-nav-index={collapseIndex}
            className={`flex w-full justify-between rounded-none hover:bg-transparent hover:text-sidebar-foreground hover:ring-0 ${
              isSidebarActive && isCollapseFocused ? "bg-transparent text-primary ring-0" : ""
            }`}
          >
            <span className="flex items-center gap-2">
              <Icon className="size-4 shrink-0" />
              {label}
            </span>
            <ChevronDown className="transition-transform group-data-[state=open]/collapsible:rotate-180" />
          </SidebarMenuButton>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <SidebarMenuSub className="mr-0 pr-0">
            {items.map((item) => (
              <LibraryNavSubItem
                key={`${section}:${item.value}`}
                section={section}
                item={item}
                filter={filter}
                onSelectItem={onSelectItem}
              />
            ))}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  );
}

interface MainNavigationProps {
  baseIndex: number;
  registerCallbacks: (callbacks: (() => void)[]) => void;
}

export const MainNavigation = ({ baseIndex, registerCallbacks }: MainNavigationProps) => {
  const { data: menu } = useLibraryMenuItems();
  const { setOpen } = useSidebar();
  const isMobile = useIsMobile();
  const { setLibraryFilter, ...filter } = useLibraryFilter();
  const { focus } = useMenuFocus();
  const navigate = useNavigate();
  const { setScrollContainer } = usePersistentScroll("sidebar");
  const [openBySection, setOpenBySection] = useSidebarSectionsOpen();

  const selectMenuItem = useCallback(
    (section: LibraryMenuSection, item: LibraryMenuItem) => {
      setLibraryFilter((current) => ({
        ...libraryFilterFromMenuSelection(section, item),
        status: current.status,
        transcript_source: current.transcript_source,
      }));
      if (isMobile) {
        setOpen(false);
      }
      navigate("/");
    },
    [isMobile, navigate, setLibraryFilter, setOpen],
  );

  const visibleSections = useMemo(() => {
    if (!menu) {
      return [];
    }

    return NAV_SECTIONS.map((config) => ({
      ...config,
      visibleItems: menu[config.section].filter(({ count }) => count > 0n),
    })).filter(({ visibleItems }) => visibleItems.length > 0);
  }, [menu]);

  const rows = useMemo<SidebarNavRow[]>(() => {
    const metadata = visibleSections.find(({ section }) => section === "no_metadata");
    const metadataRows = metadata
      ? [
          { kind: "collapse" as const, section: metadata.section },
          ...(openBySection.no_metadata
            ? metadata.visibleItems.map(({ value }) => ({
                kind: "item" as const,
                section: metadata.section,
                value,
              }))
            : []),
        ]
      : [];
    const hotRows =
      visibleSections
        .find(({ section }) => section === "hot")
        ?.visibleItems.flatMap(({ value }) => [
          { kind: "item" as const, section: "hot" as const, value },
          ...(value === "analysed" ? metadataRows : []),
        ]) ?? [];
    const libraryRows = visibleSections
      .filter(({ section }) => !["hot", "no_metadata", "playlists"].includes(section))
      .flatMap(({ section, visibleItems }) => {
        const sectionRows: SidebarNavRow[] = [{ kind: "collapse", section }];
        if (openBySection[section]) {
          sectionRows.push(
            ...visibleItems.map(({ value }) => ({ kind: "item" as const, section, value })),
          );
        }
        return sectionRows;
      });
    const playlistRows = visibleSections
      .filter(({ section }) => section === "playlists")
      .flatMap(({ section, visibleItems }) =>
        visibleItems.map(({ value }) => ({ kind: "item" as const, section, value })),
      );
    return [...hotRows, ...libraryRows, { kind: "route", id: "folders" }, ...playlistRows];
  }, [visibleSections, openBySection]);

  useEffect(() => {
    const callbacks = rows.map((row) => {
      if (row.kind === "collapse") {
        return () => {
          setOpenBySection((prev) => ({ ...prev, [row.section]: !prev[row.section] }));
        };
      }

      if (row.kind === "route") {
        return () => {
          navigate("/folders");
          if (isMobile) setOpen(false);
        };
      }

      return () => {
        const item = menu?.[row.section].find((entry) => entry.value === row.value);
        if (!item) {
          return;
        }
        selectMenuItem(row.section, item);
      };
    });

    registerCallbacks(callbacks);

    return () => {
      registerCallbacks([]);
    };
  }, [rows, menu, selectMenuItem, registerCallbacks, navigate, isMobile, setOpen]);

  const isSidebarActive = focus.active && focus.panel === "sidebar";

  useEffect(() => {
    if (!isSidebarActive || focus.source === "mouse") {
      return;
    }

    const rafId = requestAnimationFrame(() => {
      const focusedItem = document.querySelector<HTMLElement>(
        `[data-sidebar-nav-index="${focus.sidebarIndex}"]`,
      );
      focusedItem?.scrollIntoView({ block: "nearest" });
    });

    return () => cancelAnimationFrame(rafId);
  }, [focus.sidebarIndex, focus.source, isSidebarActive, rows]);

  const showEmptyPlaceholder = !menu;
  const hotSection = visibleSections.find(({ section }) => section === "hot");
  const metadataSection = visibleSections.find(({ section }) => section === "no_metadata");
  const librarySections = visibleSections.filter(
    ({ section }) => !["hot", "no_metadata", "playlists"].includes(section),
  );
  const playlistSection = visibleSections.find(({ section }) => section === "playlists");

  return (
    <SidebarContent className="overflow-hidden">
      <div
        ref={setScrollContainer}
        className="no-scrollbar min-h-0 flex-1 overflow-auto overscroll-contain"
      >
        <SidebarGroup className="px-2">
          <SidebarMenu>
            <SidebarNavProvider rows={rows} baseIndex={baseIndex}>
              {showEmptyPlaceholder ? (
                <SidebarMenuItem className="px-1 py-2">
                  <div className="space-y-1">
                    <SidebarMenuSkeleton showIcon />
                    <SidebarMenuSkeleton showIcon />
                    <SidebarMenuSkeleton showIcon />
                  </div>
                </SidebarMenuItem>
              ) : (
                <>
                  <li className="mb-1 px-2 text-[8px] font-medium uppercase tracking-[0.12em] text-sidebar-foreground/42">
                    Browse
                  </li>
                  {hotSection?.visibleItems.map((item) => (
                    <Fragment key={`hot:${item.value}`}>
                      <FlatLibraryNavItem
                        section="hot"
                        item={item}
                        filter={filter}
                        icon={HOT_ITEM_ICONS[item.value]}
                        onSelectItem={selectMenuItem}
                      />
                      {item.value === "analysed" && metadataSection ? (
                        <LibraryNavSection
                          section={metadataSection.section}
                          label={metadataSection.label}
                          icon={metadataSection.icon}
                          items={metadataSection.visibleItems}
                          filter={filter}
                          open={openBySection.no_metadata}
                          onToggleOpen={(open) =>
                            setOpenBySection((prev) => ({ ...prev, no_metadata: open }))
                          }
                          onSelectItem={selectMenuItem}
                        />
                      ) : null}
                    </Fragment>
                  ))}

                  <li className="mb-1 mt-5 px-2 text-[8px] font-medium uppercase tracking-[0.12em] text-sidebar-foreground/42">
                    My library
                  </li>
                  {librarySections.map((config) => (
                    <LibraryNavSection
                      key={config.section}
                      section={config.section}
                      label={config.label}
                      icon={config.icon}
                      items={config.visibleItems}
                      filter={filter}
                      open={openBySection[config.section]}
                      onToggleOpen={(open) =>
                        setOpenBySection((prev) => ({ ...prev, [config.section]: open }))
                      }
                      onSelectItem={selectMenuItem}
                    />
                  ))}
                </>
              )}
              <FoldersNavItem
                onSelect={() => {
                  navigate("/folders");
                  if (isMobile) setOpen(false);
                }}
              />

              {!showEmptyPlaceholder && playlistSection ? (
                <>
                  <li className="mb-1 mt-5 px-2 text-[8px] font-medium uppercase tracking-[0.12em] text-sidebar-foreground/42">
                    Playlists
                  </li>
                  {playlistSection.visibleItems.map((item) => (
                    <FlatLibraryNavItem
                      key={`playlists:${item.value}`}
                      section="playlists"
                      item={item}
                      filter={filter}
                      onSelectItem={selectMenuItem}
                    />
                  ))}
                </>
              ) : null}
            </SidebarNavProvider>
          </SidebarMenu>
        </SidebarGroup>
      </div>
    </SidebarContent>
  );
};
