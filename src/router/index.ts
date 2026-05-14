import {
  createRouter,
  createWebHashHistory,
  type RouteRecordRaw,
} from "vue-router";
import Home from "@/views/Home.vue";
import Design from "@/views/Design.vue";
import Preview from "@/views/Preview.vue";
import Export from "@/views/Export.vue";
import Runs from "@/views/Runs.vue";
import Import from "@/views/Import.vue";
import Instruct from "@/views/Instruct.vue";
import Charuco from "@/views/Charuco.vue";
import Photoplan from "@/views/Photoplan.vue";
import Method from "@/views/Method.vue";

export const routes: RouteRecordRaw[] = [
  { path: "/", name: "home", component: Home },
  {
    path: "/projects/:id/design",
    name: "design",
    component: Design,
    props: true,
  },
  {
    path: "/projects/:id/method",
    name: "method",
    component: Method,
    props: true,
  },
  {
    path: "/projects/:id/preview",
    name: "preview",
    component: Preview,
    props: true,
  },
  {
    path: "/projects/:id/export",
    name: "export",
    component: Export,
    props: true,
  },
  {
    path: "/projects/:id/runs",
    name: "runs",
    component: Runs,
    props: true,
  },
  {
    path: "/projects/:id/import",
    name: "import",
    component: Import,
    props: true,
  },
  {
    path: "/projects/:id/instruct",
    name: "instruct",
    component: Instruct,
    props: true,
  },
  {
    path: "/projects/:id/charuco",
    name: "charuco",
    component: Charuco,
    props: true,
  },
  {
    path: "/projects/:id/photoplan",
    name: "photoplan",
    component: Photoplan,
    props: true,
  },
];

export default createRouter({
  history: createWebHashHistory(),
  routes,
});
