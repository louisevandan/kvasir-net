import { createContext, useContext } from 'react'

export type Route = 'overview' | 'wallet' | 'node' | 'map' | 'nodeSettings' | 'inference' | 'models' | 'pricing' | 'settings'
export const NavCtx = createContext<{ route: Route; go: (r: Route) => void }>({ route: 'overview', go: () => {} })
export const useNav = () => useContext(NavCtx)
