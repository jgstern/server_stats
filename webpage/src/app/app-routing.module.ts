import { NgModule } from '@angular/core';
import { RouterModule, Routes } from '@angular/router';
import { FaqComponent } from './faq/faq.component';
import { LinkFinderComponent } from './link-finder/link-finder.component';
import { RoomListComponent } from './room-list/room-list.component';
import { ThreeDGraphComponent } from './three-d-graph/three-d-graph.component';

const routes: Routes = [
  { path: 'links', component: LinkFinderComponent },
  { path: '3d', component: ThreeDGraphComponent },
  { path: 'faq', component: FaqComponent },
  { path: '**', component: RoomListComponent, pathMatch: 'full' }
];

@NgModule({
  imports: [RouterModule.forRoot(routes)],
  exports: [RouterModule]
})
export class AppRoutingModule { }
