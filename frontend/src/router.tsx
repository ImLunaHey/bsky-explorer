/* eslint-disable react-refresh/only-export-components */
import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import App from './App'
import type { SearchKind } from './types'

const rootRoute=createRootRoute()
const musicRoute=createRoute({getParentRoute:()=>rootRoute,path:'/music',component:()=> <App route={{view:'music'}}/>})
const indexRoute=createRoute({getParentRoute:()=>rootRoute,path:'/',component:()=> <App route={{view:'discover'}}/>})
const networkRoute=createRoute({getParentRoute:()=>rootRoute,path:'/network',component:()=> <App route={{view:'network'}}/>})
const exploreRoute=createRoute({getParentRoute:()=>rootRoute,path:'/explore',validateSearch:(search:Record<string,unknown>)=>({topic:typeof search.topic==='string' && ['music','writing','films','events','bookmarks','streams','games'].includes(search.topic)?search.topic:''}),component:ExploreRoute})
const appsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/apps',component:()=> <App route={{view:'apps'}}/>})
const appRoute=createRoute({getParentRoute:()=>rootRoute,path:'/apps/$appId',component:AppRoute})
const storyRoute=createRoute({getParentRoute:()=>rootRoute,path:'/stories/$uri',component:StoryRoute})
const musicDetailRoute=createRoute({getParentRoute:()=>rootRoute,path:'/music/$id',component:MusicDetailRoute})
const advancedRoute=createRoute({getParentRoute:()=>rootRoute,path:'/advanced',component:()=> <App route={{view:'advanced'}}/>})
const adminRoute=createRoute({getParentRoute:()=>rootRoute,path:'/admin',component:()=> <App route={{view:'admin'}}/>})
const firehoseRoute=createRoute({getParentRoute:()=>rootRoute,path:'/firehose',component:()=> <App route={{view:'firehose'}}/>})
const firehoseImagesRoute=createRoute({getParentRoute:()=>rootRoute,path:'/firehose/images',component:()=> <App route={{view:'firehose-images'}}/>})
const detectionsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/detections',component:()=> <App route={{view:'anomalies'}}/>})
const systemRoute=createRoute({getParentRoute:()=>rootRoute,path:'/system',component:()=> <App route={{view:'system'}}/>})
const diagnosticsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/system/network',component:()=> <App route={{view:'diagnostics'}}/>})
const backfillsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/backfills',component:()=> <App route={{view:'backfills'}}/>})
const identitiesRoute=createRoute({getParentRoute:()=>rootRoute,path:'/identities',component:()=> <App route={{view:'identities'}}/>})
const identityRoute=createRoute({getParentRoute:()=>rootRoute,path:'/identities/$jobId',component:IdentityRoute})
const crawlRoute=createRoute({getParentRoute:()=>rootRoute,path:'/crawls/$crawlId',component:CrawlRoute})
const coverageRoute=createRoute({getParentRoute:()=>rootRoute,path:'/coverage',component:()=> <App route={{view:'coverage'}}/>})
const collectionsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/collections',validateSearch:(search:Record<string,unknown>)=>({collection:typeof search.collection==='string'?search.collection:''}),component:CollectionsRoute})
const communitiesRoute=createRoute({getParentRoute:()=>rootRoute,path:'/communities',component:()=> <App route={{view:'communities'}}/>})
const communityRoute=createRoute({getParentRoute:()=>rootRoute,path:'/communities/$communityId',component:CommunityRoute})
const backfillRoute=createRoute({getParentRoute:()=>rootRoute,path:'/backfills/$jobId',component:BackfillRoute})
const searchRoute=createRoute({getParentRoute:()=>rootRoute,path:'/search',validateSearch:(search:Record<string,unknown>)=>({q:typeof search.q==='string'?search.q:'',type:(['all','account','record','event'].includes(String(search.type))?search.type:'all') as SearchKind,offset:Math.max(0,Number(search.offset)||0)}),component:SearchRoute})
const accountRoute=createRoute({getParentRoute:()=>rootRoute,path:'/accounts/$did',component:AccountRoute})
const recordRoute=createRoute({getParentRoute:()=>rootRoute,path:'/records/$uri',component:RecordRoute})
const clusterRoute=createRoute({getParentRoute:()=>rootRoute,path:'/clusters/$clusterId',component:ClusterRoute})
const trendRoute=createRoute({getParentRoute:()=>rootRoute,path:'/trends/$dimension',validateSearch:(search:Record<string,unknown>)=>({key:typeof search.key==='string'?search.key:''}),component:TrendRoute})

function AccountRoute(){const {did}=accountRoute.useParams();return <App route={{view:'account',did}}/>}
function ExploreRoute(){const {topic}=exploreRoute.useSearch();return <App route={{view:'explore',topic}}/>}
function AppRoute(){const {appId}=appRoute.useParams();return <App route={{view:'app',appId}}/>}
function StoryRoute(){const {uri}=storyRoute.useParams();return <App route={{view:'story',uri}}/>}
function MusicDetailRoute(){const {id}=musicDetailRoute.useParams();return <App route={{view:'music-detail',id}}/>}
function CollectionsRoute(){const {collection}=collectionsRoute.useSearch();return <App route={{view:'collections',collection}}/>}
function RecordRoute(){const {uri}=recordRoute.useParams();return <App route={{view:'record',uri}}/>}
function ClusterRoute(){const {clusterId}=clusterRoute.useParams();return <App route={{view:'cluster',clusterId}}/>}
function TrendRoute(){const {dimension}=trendRoute.useParams();const {key}=trendRoute.useSearch();return <App route={{view:'trend',dimension,trendKey:key}}/>}
function SearchRoute(){const {q,type,offset}=searchRoute.useSearch();return <App route={{view:'search',query:q,searchType:type,offset}}/>}
function BackfillRoute(){const {jobId}=backfillRoute.useParams();return <App route={{view:'backfill',jobId}}/>}
function IdentityRoute(){const {jobId}=identityRoute.useParams();return <App route={{view:'identity',jobId}}/>}
function CrawlRoute(){const {crawlId}=crawlRoute.useParams();return <App route={{view:'crawl',crawlId}}/>}
function CommunityRoute(){const {communityId}=communityRoute.useParams();return <App route={{view:'community',communityId}}/>}

const routeTree=rootRoute.addChildren([indexRoute,networkRoute,exploreRoute,appsRoute,appRoute,storyRoute,musicDetailRoute,advancedRoute,adminRoute,musicRoute,collectionsRoute,firehoseRoute,firehoseImagesRoute,searchRoute,coverageRoute,communitiesRoute,communityRoute,backfillsRoute,backfillRoute,crawlRoute,identitiesRoute,identityRoute,detectionsRoute,systemRoute,diagnosticsRoute,accountRoute,recordRoute,clusterRoute,trendRoute])
export const router=createRouter({routeTree,defaultPreload:'intent'})

declare module '@tanstack/react-router' { interface Register { router: typeof router } }
