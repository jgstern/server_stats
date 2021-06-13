import { AfterViewInit, Component, OnInit, ViewChild } from '@angular/core';
import { MatPaginator } from '@angular/material/paginator';
import { MatSort } from '@angular/material/sort';
import { MatTableDataSource } from '@angular/material/table';
import Autolinker from 'autolinker';
import { ApiService, Row } from '../api.service';

@Component({
  selector: 'app-space-finder',
  templateUrl: './space-finder.component.html',
  styleUrls: ['./space-finder.component.scss']
})
export class SpaceFinderComponent implements OnInit, AfterViewInit {

  title = 'server-stats';
  filterColumn = 'name';
  private rows: Row[] = [];
  dataSource = new MatTableDataSource<Row>([]);
  displayedColumns: string[] = ['name', 'alias', 'room_id', 'topic', 'members', 'incoming_links', 'outgoing_links'];
  private first = true;
  resultsLength = 0;
  isLoadingResults = true;
  private filterValues = {
    name: '',
    alias: '',
    topic: '',
  };

  @ViewChild(MatPaginator, { static: true })
  private paginator!: MatPaginator;
  @ViewChild(MatSort, { static: true })
  private sort!: MatSort;

  constructor(public api: ApiService) {
    this.dataSource.filterPredicate = this.tableFilter();
  }

  ngAfterViewInit(): void {
    this.dataSource.paginator = this.paginator;
    this.dataSource.sort = this.sort;
  }

  ngOnInit(): void {
    if (this.api.data != null && this.api.data != undefined) {
      const data = this.api.data;
      if (data.nodes != null && data.nodes != undefined) {
        const nodes = data.nodes.filter(room => {
          return room.is_space;
        });
        if (this.first) {
          this.rows = nodes;
          this.rows = this.rows.map(data => {
            if (data.updated === false || data.updated == null) {
              const alias_server = data.alias.split(":")[1];
              data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;

          });
          this.first = false;
          this.resultsLength = this.rows.length;
          this.isLoadingResults = false;
          this.dataSource.data = this.rows;
          this.dataSource.sort = this.sort;
          this.dataSource.paginator = this.paginator;
        }
      }

    }
    this.api.getDataUpdates().subscribe(data => {
      if (data != null) {
        const nodes = data.nodes.filter(room => {
          return room.is_space;
        });
        if (this.first) {
          this.rows = nodes.filter(room => {
            return room.is_space;
          });
          this.rows = this.rows.map(data => {
            if (data.updated === false || data.updated == null) {
              const alias_server = data.alias.split(":")[1];
              data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;

          });
          this.first = false;
          this.resultsLength = this.rows.length;
          this.isLoadingResults = false;
          this.dataSource.data = this.rows;
          this.dataSource.sort = this.sort;
          this.dataSource.paginator = this.paginator;
        }
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

  updateFilter(event: Event) {
    const filterValue = (event.target as HTMLInputElement).value;
    if (this.filterColumn == "name") {
      this.filterValues.name = filterValue.trim().toLowerCase();
      this.dataSource.filter = JSON.stringify(this.filterValues);
    } else if (this.filterColumn == "alias") {
      this.filterValues.alias = filterValue.trim().toLowerCase();
      this.dataSource.filter = JSON.stringify(this.filterValues);
    } else if (this.filterColumn == "topic") {
      this.filterValues.topic = filterValue.trim().toLowerCase();
      this.dataSource.filter = JSON.stringify(this.filterValues);
    }
    if (this.dataSource.paginator) {
      this.dataSource.paginator.firstPage();
    }
  }


  tableFilter(): (data: any, filter: string) => boolean {
    let filterFunction = (data: { name: string; alias: string; topic: string; }, filter: string) => {
      let searchTerms = JSON.parse(filter);
      return data.name.toLowerCase().indexOf(searchTerms.name) !== -1
        && data.alias.toLowerCase().indexOf(searchTerms.alias) !== -1
        && data.topic.toLowerCase().indexOf(searchTerms.topic) !== -1;
    }
    return filterFunction;
  }
}
