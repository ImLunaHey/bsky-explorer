/* eslint-disable react-refresh/only-export-components */
import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import App from './App'

const rootRoute=createRootRoute()
const indexRoute=createRoute({getParentRoute:()=>rootRoute,path:'/',component:()=> <App route={{view:'network'}}/>})
const firehoseRoute=createRoute({getParentRoute:()=>rootRoute,path:'/firehose',component:()=> <App route={{view:'firehose'}}/>})
const detectionsRoute=createRoute({getParentRoute:()=>rootRoute,path:'/detections',component:()=> <App route={{view:'anomalies'}}/>})
const systemRoute=createRoute({getParentRoute:()=>rootRoute,path:'/system',component:()=> <App route={{view:'system'}}/>})
const accountRoute=createRoute({getParentRoute:()=>rootRoute,path:'/accounts/$did',component:AccountRoute})
const recordRoute=createRoute({getParentRoute:()=>rootRoute,path:'/records/$uri',component:RecordRoute})
const clusterRoute=createRoute({getParentRoute:()=>rootRoute,path:'/clusters/$clusterId',component:ClusterRoute})

function AccountRoute(){const {did}=accountRoute.useParams();return <App route={{view:'account',did}}/>}
function RecordRoute(){const {uri}=recordRoute.useParams();return <App route={{view:'record',uri}}/>}
function ClusterRoute(){const {clusterId}=clusterRoute.useParams();return <App route={{view:'cluster',clusterId}}/>}

const routeTree=rootRoute.addChildren([indexRoute,firehoseRoute,detectionsRoute,systemRoute,accountRoute,recordRoute,clusterRoute])
export const router=createRouter({routeTree,defaultPreload:'intent'})

declare module '@tanstack/react-router' { interface Register { router: typeof router } }
