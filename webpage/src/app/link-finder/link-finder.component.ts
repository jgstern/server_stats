import { Component, OnInit, ViewChild } from '@angular/core';
import { ColumnMode, DatatableComponent } from '@swimlane/ngx-datatable';

@Component({
  selector: 'app-link-finder',
  templateUrl: './link-finder.component.html',
  styleUrls: ['./link-finder.component.scss']
})
export class LinkFinderComponent implements OnInit {
  @ViewChild(DatatableComponent)
  table!: DatatableComponent;

  rows: any[] = [];
  temp: any[] = [];
  filterColumn = 'name';
  columns = [{ prop: 'name', name: 'Roomname' }, { name: 'Alias' }, { prop: 'room_id', name: 'Room ID' }, { name: 'Topic' }, { prop: 'incoming_links', name: 'Incoming Links' }, { prop: 'outgoing_links', name: 'Outgoing Links' }];
  ColumnMode = ColumnMode;

  constructor() { }

  ngOnInit(): void {
  }

  updateSelection(value: string) {
    this.filterColumn = value.toLowerCase();
  }

  updateFilter(event: any) {
    const val = event.target.value.toLowerCase();
    const filterColumn = this.filterColumn;
    // filter our data
    const temp = this.temp.filter(function (d) {
      const indexName = filterColumn as "id" | "name" | "alias" | "avatar" | "topic" | "weight";
      return d[indexName].toLowerCase().indexOf(val) !== -1 || !val;
    });

    // update the rows
    this.rows = temp;
    // Whenever the filter changes, always go back to the first page
    this.table.offset = 0;
  }

}
