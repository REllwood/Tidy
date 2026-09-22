import { createContext, useContext } from "react";

/**
 * The page mutations a tree row can run, hoisted to the tree so hundreds of
 * rows don't each mount their own mutation hooks. Every member is stable, so
 * the memoised rows only re-render when their own data changes.
 */
export interface TreeActions {
  rename: (id: string, title: string) => void;
  setFavourite: (id: string, isFavorite: boolean) => void;
  remove: (id: string) => void;
  addSubpage: (parentId: string) => void;
  /** Would dropping the page being dragged on this row do anything? */
  canDrop: (activeId: string | null, targetId: string) => boolean;
}

const TreeActionsContext = createContext<TreeActions | null>(null);

export const TreeActionsProvider = TreeActionsContext.Provider;

export function useTreeActions(): TreeActions {
  const actions = useContext(TreeActionsContext);
  if (!actions)
    throw new Error("useTreeActions must be used inside the page tree");
  return actions;
}
