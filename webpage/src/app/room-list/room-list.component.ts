import { Component, OnInit, ViewChild } from '@angular/core';
import { ColumnMode, DatatableComponent } from '@swimlane/ngx-datatable';
import Autolinker from 'autolinker';
import { ApiService, Row } from '../api.service';
import Fuse from 'fuse.js';


@Component({
  selector: 'app-room-list',
  templateUrl: './room-list.component.html',
  styleUrls: ['./room-list.component.scss']
})
export class RoomListComponent implements OnInit {
  @ViewChild(DatatableComponent)
  table!: DatatableComponent;

  title = 'server-stats';
  filterColumn = 'name';
  rows: Row[] = [];
  temp: Row[] = [];
  columns = [{ prop: 'name', name: 'Roomname' }, { name: 'Alias' }, { prop: 'room_id', name: 'Room ID' }, { name: 'Topic' }, { prop: 'incoming_links', name: 'Incoming Links' }, { prop: 'outgoing_links', name: 'Outgoing Links' }];
  ColumnMode = ColumnMode;

  constructor(public api: ApiService) { }

  ngOnInit(): void {
    if (this.api.data != null && this.api.data != undefined) {
      const data = this.api.data;
      this.rows = data.nodes;
      if (data.nodes != null && data.nodes != undefined) {
        let nodes = data.nodes;
        this.temp = nodes;
        this.rows = nodes;
      }

    }
    this.api.getDataUpdates().subscribe(data => {
      if (data != null && data != undefined) {
        let nodes = data.nodes;
        this.temp = nodes;
        this.rows = nodes;
      }
    })
  }

  truncateText(text: string, length: number) {
    if (text.length <= length) {
      return text;
    }

    return text.substr(0, length) + '\u2026';
  }

  updateSelection(value: string) {
    this.filterColumn = value.toLowerCase();
  }

  updateFilter(event: any) {
    const val = event.target.value.toLowerCase();
    const filterColumn = this.filterColumn;
    // filter our data
    const indexName = filterColumn as "id" | "name" | "alias" | "avatar" | "topic" | "weight";
    const options = {
      includeScore: true,
      // Search in `author` and in `tags` array
      keys: [indexName]
    }

    const fuse = new Fuse(this.temp, options);

    const result = fuse.search(val);
    if (result.length == 0) {
      if (this.api.data != null) {
        const nodes = this.api.data.nodes.map(data => {
          if (data.updated === false) {
            data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
            data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
            data.updated = true;
          }
          return data;

        });
        this.rows = nodes;
      }
      return;
    }

    result.sort((a, b) => {
      // Compare the 2 scores
      if (a.score!! < b.score!!) return -1;
      if (a.score!! > b.score!!) return 1;
      return 0;
    });

    const temp = result.map(e => e.item);
    // update the rows
    this.rows = temp.map(data => {
      if (data.updated === false) {
        data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
        data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
        data.updated = true;
      }
      return data;
    });
    // Whenever the filter changes, always go back to the first page
    this.table.offset = 0;
  }
}
