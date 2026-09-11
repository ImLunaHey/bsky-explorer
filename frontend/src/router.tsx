/* eslint-disable react-refresh/only-export-components */
import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import App from './App'
import type { SearchKind } from './types'

const rootRoute=createRootRoute()
const indexRoute=createRoute({getParentRoute:()=>rootRoute,path:'/',component:()=> <App route={{view:'network'}}/>})
const firehoseRoute=createRoute({getParentRoute:()=>rootRoute,path:'/firehose',component:()=> <App route={{view:'firehose'}}/>})
const detectionsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/detections',component:()=> <App route={{view:'anomalies'}}/>})
const systemRoute=createRoute({getParentRoute:()=>rootRoute,path:'/system',component:()=> <App route={{view:'system'}}/>})
const backfillsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/backfills',component:()=> <App route={{view:'backfills'}}/>})
const identitiesRoute=createRoute({getParentRoute:()=>rootRoute,path:'/identities',component:()=> <App route={{view:'identities'}}/>})
const identityRoute=createRoute({getParentRoute:()=>rootRoute,path:'/identities/$jobId',component:IdentityRoute})
const backfillRoute=createRoute({getParentRoute:()=>rootRoute,path:'/backfills/$jobId',component:BackfillRoute})
const searchRoute=createRoute({getParentRoute:()=>rootRoute,path:'/search',validateSearch:(search:Record<string,unknown>)=>({q:typeof search.q==='string'?search.q:'',type:(['all','account','record','event'].includes(String(search.type))?search.type:'all') as SearchKind,offset:Math.max(0,Number(search.offset)||0)}),component:SearchRoute})
const accountRoute=createRoute({getParentRoute:()=>rootRoute,path:'/accounts/$did',component:AccountRoute})
const recordRoute=createRoute({getParentRoute:()=>rootRoute,path:'/records/$uri',component:RecordRoute})
const clusterRoute=createRoute({getParentRoute:()=>rootRoute,path:'/clusters/$clusterId',component:ClusterRoute})
const trendRoute=createRoute({getParentRoute:()=>rootRoute,path:'/trends/$dimension',validateSearch:(search:Record<string,unknown>)=>({key:typeof search.key==='string'?search.key:''}),component:TrendRoute})

function AccountRoute(){const {did}=accountRoute.useParams();return <App route={{view:'account',did}}/>}
function RecordRoute(){const {uri}=recordRoute.useParams();return <App route={{view:'record',uri}}/>}
function ClusterRoute(){const {clusterId}=clusterRoute.useParams();return <App route={{view:'cluster',clusterId}}/>}
function TrendRoute(){const {dimension}=trendRoute.useParams();const {key}=trendRoute.useSearch();return <App route={{view:'trend',dimension,trendKey:key}}/>}
function SearchRoute(){const {q,type,offset}=searchRoute.useSearch();return <App route={{view:'search',query:q,searchType:type,offset}}/>}
function BackfillRoute(){const {jobId}=backfillRoute.useParams();return <App route={{view:'backfill',jobId}}/>}
function IdentityRoute(){const {jobId}=identityRoute.useParams();return <App route={{view:'identity',jobId}}/>}

const routeTree=rootRoute.addChildren([indexRoute,firehoseRoute,searchRoute,backfillsRoute,backfillRoute,identitiesRoute,identityRoute,detectionsRoute,systemRoute,accountRoute,recordRoute,clusterRoute,trendRoute])
export const router=createRouter({routeTree,defaultPreload:'intent'})

declare module '@tanstack/react-router' { interface Register { router: typeof router } }
