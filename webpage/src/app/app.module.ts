import { NgModule } from '@angular/core';
import { BrowserModule } from '@angular/platform-browser';

import { AppRoutingModule } from './app-routing.module';
import { AppComponent } from './app.component';
import { NZ_I18N } from 'ng-zorro-antd/i18n';
import { en_US } from 'ng-zorro-antd/i18n';
import { registerLocaleData } from '@angular/common';
import en from '@angular/common/locales/en';
import { FormsModule } from '@angular/forms';
import { HttpClientModule } from '@angular/common/http';
import { BrowserAnimationsModule } from '@angular/platform-browser/animations';
import { NzLayoutModule } from 'ng-zorro-antd/layout';
import { NzBreadCrumbModule } from 'ng-zorro-antd/breadcrumb';
import { NzMenuModule } from 'ng-zorro-antd/menu';
import { NgxDatatableModule } from '@swimlane/ngx-datatable';
import { NzInputModule } from 'ng-zorro-antd/input';
import { NzIconModule } from 'ng-zorro-antd/icon';
import { NzSelectModule } from 'ng-zorro-antd/select';
import { NzTypographyModule } from 'ng-zorro-antd/typography';
import { LinkFinderComponent } from './link-finder/link-finder.component';
import { RoomListComponent } from './room-list/room-list.component';
import { LinkyModule } from 'ngx-linky';
import { ThreeDGraphComponent } from './three-d-graph/three-d-graph.component';
import { FaqComponent } from './faq/faq.component';
import { SpaceFinderComponent } from './space-finder/space-finder.component';

registerLocaleData(en);

@NgModule({
  declarations: [
    AppComponent,
    LinkFinderComponent,
    RoomListComponent,
    ThreeDGraphComponent,
    FaqComponent,
    SpaceFinderComponent
  ],
  imports: [
    BrowserModule,
    AppRoutingModule,
    FormsModule,
    HttpClientModule,
    BrowserAnimationsModule,
    NzLayoutModule,
    NzBreadCrumbModule,
    NzMenuModule,
    NgxDatatableModule,
    NzInputModule,
    NzIconModule,
    NzSelectModule,
    NzTypographyModule,
    LinkyModule
  ],
  providers: [{ provide: NZ_I18N, useValue: en_US }],
  bootstrap: [AppComponent]
})
export class AppModule { }
